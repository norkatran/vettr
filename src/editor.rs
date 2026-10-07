//! Editor command templates (port of `src/shared/editor.ts`).
//!
//! The TS splits the template with the regex `"[^"]*"|\S+` rather than shell rules (a lone quote is
//! a literal word), so the splitting is done by hand here to keep that behaviour exactly; the
//! `shell-words` crate would reject an unmatched quote.

/// A program and its arguments, ready for `std::process::Command`.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorCommand {
    pub command: String,
    pub args: Vec<String>,
}

/// Split on whitespace; a double-quoted run (with a closing quote) is one word, quotes removed.
fn split_template(template: &str) -> Vec<String> {
    let chars: Vec<char> = template.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let mut end = i;
        if chars[i] == '"' {
            // `"[^"]*"` first
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '"' {
                j += 1;
            }
            if j < chars.len() {
                end = j + 1;
            }
        }
        if end == i {
            // `\S+`
            end = i;
            while end < chars.len() && !chars[end].is_whitespace() {
                end += 1;
            }
        }
        let word: Vec<char> = chars[i..end].to_vec();
        let stripped: String = if word.len() >= 2 && word[0] == '"' && word[word.len() - 1] == '"' {
            word[1..word.len() - 1].iter().collect()
        } else {
            word.iter().collect()
        };
        words.push(stripped);
        i = end;
    }
    words
}

/// Replace `{file}`, `{line}` and `{project}` in one pass (inserted text is never rescanned).
fn substitute(word: &str, file: &str, line: &str, project: &str) -> String {
    let mut out = String::new();
    let mut rest: &str = word;
    while let Some(pos) = rest.find('{') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        if let Some(t) = tail.strip_prefix("{file}") {
            out.push_str(file);
            rest = t;
        } else if let Some(t) = tail.strip_prefix("{line}") {
            out.push_str(line);
            rest = t;
        } else if let Some(t) = tail.strip_prefix("{project}") {
            out.push_str(project);
            rest = t;
        } else {
            out.push('{');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Turn an editor command template (for example `code -g {file}:{line}`) into a command and
/// arguments. The template is split on whitespace (double quotes group words) before `{file}`,
/// `{line}` and `{project}` are substituted, so a path with spaces or shell characters stays one
/// argument and no shell is involved. Returns `None` when the template is empty.
pub fn build_editor_command(
    template: &str,
    file: &str,
    line: u32,
    project: &str,
) -> Option<EditorCommand> {
    let line_text = line.to_string();
    let mut words: Vec<String> = split_template(template)
        .iter()
        .map(|w| substitute(w, file, &line_text, project))
        .collect();
    if words.is_empty() {
        return None;
    }
    let command = words.remove(0);
    if command.is_empty() {
        return None;
    }
    Some(EditorCommand {
        command,
        args: words,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(command: &str, args: &[&str]) -> Option<EditorCommand> {
        Some(EditorCommand {
            command: command.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
        })
    }

    #[test]
    fn substitutes_file_and_line_into_the_arguments() {
        assert_eq!(
            build_editor_command("code -g {file}:{line}", "/a/b.ts", 7, "/p"),
            cmd("code", &["-g", "/a/b.ts:7"])
        );
    }

    #[test]
    fn keeps_a_path_with_spaces_as_one_argument_and_honours_quoted_words() {
        assert_eq!(
            build_editor_command("\"my editor\" --line {line} {file}", "/a b/c.ts", 2, "/p"),
            cmd("my editor", &["--line", "2", "/a b/c.ts"])
        );
    }

    #[test]
    fn keeps_a_lone_quote_character_as_a_literal_word() {
        assert_eq!(
            build_editor_command("ed \"", "/f", 1, "/p"),
            cmd("ed", &["\""])
        );
    }

    #[test]
    fn substitutes_project() {
        assert_eq!(
            build_editor_command("code {project} -g {file}:{line}", "/p/a.ts", 3, "/p"),
            cmd("code", &["/p", "-g", "/p/a.ts:3"])
        );
    }

    #[test]
    fn does_not_interpret_dollar_patterns_in_the_path() {
        assert_eq!(
            build_editor_command("ed {file}", "/a/$&.ts", 1, "/p").map(|c| c.args),
            Some(vec!["/a/$&.ts".to_string()])
        );
    }

    #[test]
    fn returns_none_for_an_empty_template() {
        assert_eq!(build_editor_command("   ", "/f", 1, "/p"), None);
    }
}
