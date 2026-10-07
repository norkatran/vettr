//! Markdown for agent prose (design 0009): a small pure parser from CommonMark text to a block
//! tree the UI can draw (`src/ui/markdown.rs`). Raw HTML is kept as plain text, never interpreted.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// A run of inline text with one style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    /// The link target. Links are drawn as styled text and are not clickable.
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(Vec<Span>),
    Heading(u8, Vec<Span>),
    Code { lang: Option<String>, text: String },
    Quote(Vec<Block>),
    List { start: Option<u64>, items: Vec<Vec<Block>> },
    Rule,
}

enum Container {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    Item(Vec<Block>),
    List { start: Option<u64>, items: Vec<Vec<Block>> },
}

#[derive(Default)]
struct Style {
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<String>,
}

fn push_block(stack: &mut [Container], block: Block) {
    match stack.last_mut() {
        Some(Container::Root(b)) | Some(Container::Quote(b)) | Some(Container::Item(b)) => {
            b.push(block)
        }
        // Blocks never arrive directly inside a list; items wrap them.
        _ => {}
    }
}

/// Close the pending inline text (tight list items have no paragraph events) as a paragraph.
fn flush(stack: &mut [Container], inline: &mut Vec<Span>) {
    if !inline.is_empty() {
        let spans = std::mem::take(inline);
        push_block(stack, Block::Paragraph(spans));
    }
}

fn push_text(inline: &mut Vec<Span>, style: &Style, text: &str, code: bool) {
    inline.push(Span {
        text: text.to_string(),
        bold: style.bold > 0,
        italic: style.italic > 0,
        strike: style.strike > 0,
        code,
        link: style.links.last().cloned(),
    });
}

/// Parse CommonMark (plus strikethrough and task lists) into blocks.
pub fn parse(source: &str) -> Vec<Block> {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut stack = vec![Container::Root(Vec::new())];
    let mut inline: Vec<Span> = Vec::new();
    let mut style = Style::default();
    let mut code: Option<(Option<String>, String)> = None;
    let mut heading: Option<u8> = None;

    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => flush(&mut stack, &mut inline),
                Tag::Heading { level, .. } => {
                    flush(&mut stack, &mut inline);
                    heading = Some(level as u8);
                }
                Tag::BlockQuote(_) => {
                    flush(&mut stack, &mut inline);
                    stack.push(Container::Quote(Vec::new()));
                }
                Tag::CodeBlock(kind) => {
                    flush(&mut stack, &mut inline);
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) if !l.is_empty() => Some(l.to_string()),
                        _ => None,
                    };
                    code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    flush(&mut stack, &mut inline);
                    stack.push(Container::List { start, items: Vec::new() });
                }
                Tag::Item => {
                    flush(&mut stack, &mut inline);
                    stack.push(Container::Item(Vec::new()));
                }
                Tag::Emphasis => style.italic += 1,
                Tag::Strong => style.bold += 1,
                Tag::Strikethrough => style.strike += 1,
                Tag::Link { dest_url, .. } => style.links.push(dest_url.to_string()),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => flush(&mut stack, &mut inline),
                TagEnd::Heading(_) => {
                    let spans = std::mem::take(&mut inline);
                    let level = heading.take().unwrap_or(1);
                    push_block(&mut stack, Block::Heading(level, spans));
                }
                TagEnd::BlockQuote(_) => {
                    flush(&mut stack, &mut inline);
                    if let Some(Container::Quote(blocks)) = stack.pop() {
                        push_block(&mut stack, Block::Quote(blocks));
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, text)) = code.take() {
                        let text = text.trim_end_matches('\n').to_string();
                        push_block(&mut stack, Block::Code { lang, text });
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Container::List { start, items }) = stack.pop() {
                        push_block(&mut stack, Block::List { start, items });
                    }
                }
                TagEnd::Item => {
                    flush(&mut stack, &mut inline);
                    if let Some(Container::Item(blocks)) = stack.pop() {
                        if let Some(Container::List { items, .. }) = stack.last_mut() {
                            items.push(blocks);
                        }
                    }
                }
                TagEnd::Emphasis => style.italic = style.italic.saturating_sub(1),
                TagEnd::Strong => style.bold = style.bold.saturating_sub(1),
                TagEnd::Strikethrough => style.strike = style.strike.saturating_sub(1),
                TagEnd::Link => {
                    style.links.pop();
                }
                _ => {}
            },
            Event::Text(t) => match code.as_mut() {
                Some((_, buf)) => buf.push_str(&t),
                None => push_text(&mut inline, &style, &t, false),
            },
            Event::Code(t) => push_text(&mut inline, &style, &t, true),
            Event::Html(t) | Event::InlineHtml(t) => match code.as_mut() {
                Some((_, buf)) => buf.push_str(&t),
                None => push_text(&mut inline, &style, &t, false),
            },
            Event::SoftBreak => push_text(&mut inline, &style, " ", false),
            Event::HardBreak => push_text(&mut inline, &style, "\n", false),
            Event::TaskListMarker(done) => {
                push_text(&mut inline, &style, if done { "\u{2611} " } else { "\u{2610} " }, false)
            }
            Event::Rule => {
                flush(&mut stack, &mut inline);
                push_block(&mut stack, Block::Rule);
            }
            _ => {}
        }
    }
    flush(&mut stack, &mut inline);
    match stack.into_iter().next() {
        Some(Container::Root(blocks)) => blocks,
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(spans: &[Span]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn paragraphs_and_inline_styles() {
        let blocks = parse("Hello **bold** and `code`.\n\nSecond *one*.");
        assert_eq!(blocks.len(), 2);
        let Block::Paragraph(spans) = &blocks[0] else { panic!() };
        assert_eq!(plain(spans), "Hello bold and code.");
        assert!(spans[1].bold && !spans[0].bold);
        assert!(spans[3].code);
        let Block::Paragraph(second) = &blocks[1] else { panic!() };
        assert!(second[1].italic);
    }

    #[test]
    fn headings_and_rules() {
        let blocks = parse("## Title\n\n---\n");
        assert!(matches!(&blocks[0], Block::Heading(2, s) if plain(s) == "Title"));
        assert_eq!(blocks[1], Block::Rule);
    }

    #[test]
    fn fenced_code_keeps_text_and_language() {
        let blocks = parse("```rust\nfn main() {}\n```\n");
        assert_eq!(
            blocks,
            vec![Block::Code { lang: Some("rust".into()), text: "fn main() {}".into() }]
        );
    }

    #[test]
    fn tight_and_nested_lists() {
        let blocks = parse("- one\n- two\n  - inner\n\n1. a\n2. b\n");
        let Block::List { start: None, items } = &blocks[0] else { panic!() };
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[1][1], Block::List { .. }));
        let Block::List { start: Some(1), items } = &blocks[1] else { panic!() };
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn quotes_links_and_html() {
        let blocks = parse("> quoted [site](https://x.dev)\n\n<b>raw</b>");
        let Block::Quote(inner) = &blocks[0] else { panic!() };
        let Block::Paragraph(spans) = &inner[0] else { panic!() };
        assert_eq!(spans[1].link.as_deref(), Some("https://x.dev"));
        assert!(plain(match &blocks[1] { Block::Paragraph(s) => s, _ => panic!() }).contains("<b>"));
    }

    #[test]
    fn empty_input_has_no_blocks() {
        assert!(parse("").is_empty());
    }
}
