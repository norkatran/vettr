//! Syntax highlighting for diff hunks (port of `src/shared/highlight.ts`, using `syntect`
//! instead of highlight.js).
//!
//! The output is theme-agnostic: each line is a list of [`Span`]s tagged with a coarse
//! [`TokenKind`], and the UI maps the kind to a colour (dark or light) when painting. Highlighting
//! a hunk costs a parse of every line, so callers should keep the result, for example in a
//! [`HighlightCache`].
//!
//! The bundled syntect syntaxes have no TypeScript, SCSS, Kotlin, TOML or Dockerfile grammar, so
//! those map to the nearest one (JavaScript, CSS, Java, none, shell). Swift and ini have none.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use crate::diff::{DiffLine, LineKind};

/// A coarse token class; the UI picks a colour for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Plain,
    Comment,
    String,
    Number,
    Keyword,
    Operator,
    Function,
    Type,
    Constant,
    Tag,
    Attribute,
    Punctuation,
}

/// A run of text in one line with the same token class.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub kind: TokenKind,
}

/// The spans of one source line (empty for an empty line).
pub type HighlightedLine = Vec<Span>;

/// Highlighting of a hunk, parallel to the hunk's lines: `None` where the line has none (unknown
/// language or highlighting failed), in which case the UI paints it as plain text.
pub type HunkHighlight = Vec<Option<HighlightedLine>>;

fn extension_key(ext: &str) -> Option<&'static str> {
    let key = match ext {
        "sh" | "bash" | "zsh" => "bash",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "css" => "css",
        "diff" | "patch" => "diff",
        "go" => "go",
        "ini" | "toml" | "conf" => "ini",
        "java" => "java",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "json" | "jsonc" => "json",
        "kt" | "kts" => "kotlin",
        "md" | "markdown" => "markdown",
        "php" => "php",
        "py" => "python",
        "rb" => "ruby",
        "rs" => "rust",
        "scss" => "scss",
        "sql" => "sql",
        "swift" => "swift",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "html" | "htm" | "vue" => "html",
        "xml" | "svg" => "xml",
        "yaml" | "yml" => "yaml",
        _ => return None,
    };
    Some(key)
}

/// The language key for a file path (for example `typescript`), or `None` when it has no known
/// grammar.
pub fn language_for(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or("").to_lowercase();
    if name == "dockerfile" || name.starts_with("dockerfile.") {
        return Some("dockerfile");
    }
    let dot = name.rfind('.')?;
    extension_key(&name[dot + 1..])
}

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The syntect syntax for a language key, falling back to the nearest bundled grammar.
fn syntax_for(language: &str) -> Option<&'static SyntaxReference> {
    let ext = match language {
        "bash" | "dockerfile" => "sh",
        "c" => "c",
        "cpp" => "cpp",
        "csharp" => "cs",
        "css" | "scss" => "css",
        "diff" => "diff",
        "go" => "go",
        "java" | "kotlin" => "java",
        "javascript" | "typescript" => "js",
        "json" => "json",
        "markdown" => "md",
        "php" => "php",
        "python" => "py",
        "ruby" => "rb",
        "rust" => "rs",
        "sql" => "sql",
        "html" => "html",
        "xml" => "xml",
        "yaml" => "yaml",
        _ => return None,
    };
    syntax_set().find_syntax_by_extension(ext)
}

/// The token class a single scope name stands for, if it says anything.
fn classify(scope: &str) -> Option<TokenKind> {
    let starts = |prefix: &str| scope.starts_with(prefix);
    if starts("comment") || starts("punctuation.definition.comment") {
        Some(TokenKind::Comment)
    } else if starts("string") || starts("punctuation.definition.string") {
        Some(TokenKind::String)
    } else if starts("constant.numeric") {
        Some(TokenKind::Number)
    } else if starts("constant") || starts("support.constant") {
        Some(TokenKind::Constant)
    } else if starts("keyword.operator") {
        Some(TokenKind::Operator)
    } else if starts("keyword") || starts("storage") || starts("variable.language") {
        Some(TokenKind::Keyword)
    } else if starts("entity.name.function")
        || starts("support.function")
        || starts("variable.function")
    {
        Some(TokenKind::Function)
    } else if starts("entity.name.tag") {
        Some(TokenKind::Tag)
    } else if starts("entity.other.attribute-name") {
        Some(TokenKind::Attribute)
    } else if starts("entity.name")
        || starts("support.type")
        || starts("support.class")
        || starts("entity.other.inherited-class")
    {
        Some(TokenKind::Type)
    } else if starts("punctuation") {
        Some(TokenKind::Punctuation)
    } else {
        None
    }
}

/// The class of the text under a scope stack: the innermost scope that classifies, except that
/// punctuation yields to an enclosing string or comment.
fn kind_of(stack: &ScopeStack, cache: &mut HashMap<Scope, Option<TokenKind>>) -> TokenKind {
    let mut punctuation = false;
    for scope in stack.as_slice().iter().rev() {
        let found = match cache.get(scope) {
            Some(k) => *k,
            None => {
                let k = classify(&scope.build_string());
                cache.insert(*scope, k);
                k
            }
        };
        match found {
            Some(TokenKind::Punctuation) => punctuation = true,
            Some(kind) => return kind,
            None => {}
        }
    }
    if punctuation {
        TokenKind::Punctuation
    } else {
        TokenKind::Plain
    }
}

fn push_span(spans: &mut Vec<Span>, text: &str, kind: TokenKind) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.kind == kind {
            last.text.push_str(text);
            return;
        }
    }
    spans.push(Span {
        text: text.to_string(),
        kind,
    });
}

/// Highlight the lines of one side as a single file. `None` if the parser fails.
fn highlight_side(syntax: &SyntaxReference, texts: &[&str]) -> Option<Vec<HighlightedLine>> {
    let set = syntax_set();
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut cache: HashMap<Scope, Option<TokenKind>> = HashMap::new();
    let mut out: Vec<HighlightedLine> = Vec::with_capacity(texts.len());
    for text in texts {
        let line = format!("{}\n", text);
        let ops = state.parse_line(&line, set).ok()?;
        let len = text.len();
        let mut cursor = 0usize;
        let mut spans: Vec<Span> = Vec::new();
        for (pos, op) in ops.iter() {
            let end = (*pos).min(len);
            if end > cursor {
                if let Some(piece) = text.get(cursor..end) {
                    let kind = kind_of(&stack, &mut cache);
                    push_span(&mut spans, piece, kind);
                }
                cursor = end;
            }
            stack.apply(op).ok()?;
        }
        if cursor < len {
            if let Some(piece) = text.get(cursor..len) {
                let kind = kind_of(&stack, &mut cache);
                push_span(&mut spans, piece, kind);
            }
        }
        out.push(spans);
    }
    Some(out)
}

/// Combine the two sides' highlighting into one entry per line. Context lines take the new-side
/// result; deletions only exist on the old side. A side that is missing, or whose line count does
/// not match its input, is discarded.
fn merge_sides(
    lines: &[DiffLine],
    old: Option<Vec<HighlightedLine>>,
    new: Option<Vec<HighlightedLine>>,
) -> HunkHighlight {
    let mut result: HunkHighlight = (0..lines.len()).map(|_| None).collect();
    let old_idx: Vec<usize> = (0..lines.len())
        .filter(|i| lines[*i].kind != LineKind::Add)
        .collect();
    let new_idx: Vec<usize> = (0..lines.len())
        .filter(|i| lines[*i].kind != LineKind::Del)
        .collect();
    if let Some(old) = old {
        if old.len() == old_idx.len() {
            for (n, idx) in old_idx.iter().enumerate() {
                if lines[*idx].kind == LineKind::Del {
                    result[*idx] = Some(old[n].clone());
                }
            }
        }
    }
    if let Some(new) = new {
        if new.len() == new_idx.len() {
            for (n, idx) in new_idx.iter().enumerate() {
                result[*idx] = Some(new[n].clone());
            }
        }
    }
    result
}

/// Highlight a hunk, returning the spans for each line (parallel to `lines`). The old side
/// (context + deletions) and the new side (context + additions) are tokenized separately so each
/// reads as coherent source. All entries are `None` for unknown languages or on failure.
pub fn highlight_hunk(path: &str, lines: &[DiffLine]) -> HunkHighlight {
    let none = || -> HunkHighlight { (0..lines.len()).map(|_| None).collect() };
    let syntax = match language_for(path).and_then(syntax_for) {
        Some(s) => s,
        None => return none(),
    };
    let old_texts: Vec<&str> = lines
        .iter()
        .filter(|l| l.kind != LineKind::Add)
        .map(|l| l.text.as_str())
        .collect();
    let new_texts: Vec<&str> = lines
        .iter()
        .filter(|l| l.kind != LineKind::Del)
        .map(|l| l.text.as_str())
        .collect();
    let old = highlight_side(syntax, &old_texts);
    let new = highlight_side(syntax, &new_texts);
    merge_sides(lines, old, new)
}

/// Keeps highlighted hunks between frames. The key is a hash of the path and the line contents,
/// so a hunk that did not change is not re-highlighted.
#[derive(Debug, Default)]
pub struct HighlightCache {
    entries: HashMap<u64, Arc<HunkHighlight>>,
}

impl HighlightCache {
    const MAX_ENTRIES: usize = 512;

    pub fn new() -> Self {
        Self::default()
    }

    fn key(path: &str, lines: &[DiffLine]) -> u64 {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        for line in lines {
            line.kind.hash(&mut hasher);
            line.text.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// The highlighting of the hunk, computed on the first call and shared afterwards.
    pub fn get(&mut self, path: &str, lines: &[DiffLine]) -> Arc<HunkHighlight> {
        let key = Self::key(path, lines);
        if let Some(found) = self.entries.get(&key) {
            return Arc::clone(found);
        }
        if self.entries.len() >= Self::MAX_ENTRIES {
            self.entries.clear();
        }
        let value = Arc::new(highlight_hunk(path, lines));
        self.entries.insert(key, Arc::clone(&value));
        value
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: LineKind, text: &str) -> DiffLine {
        DiffLine {
            kind,
            old_no: if kind == LineKind::Add { None } else { Some(1) },
            new_no: if kind == LineKind::Del { None } else { Some(1) },
            text: text.to_string(),
        }
    }

    fn has_kind(spans: &Option<HighlightedLine>, kind: TokenKind) -> bool {
        match spans {
            Some(s) => s.iter().any(|span| span.kind == kind),
            None => false,
        }
    }

    fn plain(text: &str) -> HighlightedLine {
        vec![Span {
            text: text.to_string(),
            kind: TokenKind::Plain,
        }]
    }

    #[test]
    fn language_for_maps_extensions_and_special_filenames() {
        assert_eq!(language_for("src/a/b.tsx"), Some("typescript"));
        assert_eq!(language_for("Dockerfile"), Some("dockerfile"));
        assert_eq!(language_for("x/README.MD"), Some("markdown"));
    }

    #[test]
    fn language_for_returns_none_for_unknown_or_missing_extensions() {
        assert_eq!(language_for("LICENSE"), None);
        assert_eq!(language_for("a.unknownext"), None);
        assert_eq!(language_for("dir.d/file"), None);
    }

    #[test]
    fn highlights_each_side_and_maps_results_back_to_lines() {
        let lines = vec![
            line(LineKind::Del, "const a = 1"),
            line(LineKind::Add, "const a = \"x\""),
            line(LineKind::Context, "/* start"),
        ];
        let out = highlight_hunk("f.ts", &lines);
        assert_eq!(out.len(), 3);
        assert!(has_kind(&out[0], TokenKind::Keyword));
        assert!(has_kind(&out[1], TokenKind::String));
        assert!(has_kind(&out[2], TokenKind::Comment));
    }

    #[test]
    fn spans_reassemble_the_line_text() {
        let lines = vec![line(LineKind::Add, "let x = foo(1, \"é\");")];
        let out = highlight_hunk("f.js", &lines);
        let joined: String = out[0]
            .as_ref()
            .map(|spans| spans.iter().map(|s| s.text.as_str()).collect::<String>())
            .unwrap_or_default();
        assert_eq!(joined, "let x = foo(1, \"é\");");
    }

    #[test]
    fn returns_nothing_for_unknown_languages() {
        let out = highlight_hunk("LICENSE", &[line(LineKind::Add, "x")]);
        assert_eq!(out.len(), 1);
        assert!(out.iter().all(|l| l.is_none()));
    }

    #[test]
    fn discards_output_whose_line_count_does_not_match_the_input() {
        let lines = vec![line(LineKind::Add, "x")];
        let out = merge_sides(&lines, None, Some(vec![plain("a"), plain("b")]));
        assert!(out.iter().all(|l| l.is_none()));
    }

    #[test]
    fn keeps_the_old_side_when_only_the_new_side_fails() {
        let lines = vec![
            line(LineKind::Del, "const a = 1"),
            line(LineKind::Add, "const b = 2"),
        ];
        let out = merge_sides(&lines, Some(vec![plain("const a = 1")]), None);
        assert!(out[0].is_some());
        assert!(out[1].is_none());
    }

    #[test]
    fn context_lines_take_the_new_side() {
        let lines = vec![line(LineKind::Context, "c")];
        let out = merge_sides(&lines, Some(vec![plain("old")]), Some(vec![plain("new")]));
        assert_eq!(out[0], Some(plain("new")));
    }

    #[test]
    fn cache_returns_the_same_result_for_the_same_hunk() {
        let lines = vec![line(LineKind::Add, "let a = 1;")];
        let mut cache = HighlightCache::new();
        let first = cache.get("f.js", &lines);
        let second = cache.get("f.js", &lines);
        assert!(Arc::ptr_eq(&first, &second));
    }
}
