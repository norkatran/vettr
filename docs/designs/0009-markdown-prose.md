# 0009 Markdown rendering of agent prose

Status: Fulfilled

## Problem

The agent writes its prose in Markdown, but the Session view showed it verbatim: `**bold**`, backtick spans, lists, headings and fenced code all appeared as raw characters.

## Decision

Everything the agent says that is not a tool call (which has its own rendering) is rendered as Markdown:

- Assistant text items in the transcript, and the message of an agent reply to a review comment.
- Not rendered: the user's own messages, errors, notices, tool input/output and file edit rows. Those stay plain.

Approach:

- `src/markdown.rs` (pure, unit tested) parses CommonMark with `pulldown-cmark` (strikethrough and task lists on, no default features) into a small block tree: paragraphs, headings, fenced code, quotes, nested lists, rules, with inline bold, italic, strikethrough, code and link spans.
- `src/ui/markdown.rs` draws that tree with egui `LayoutJob`s. egui has no bold face, so bold uses the strong text colour; inline code gets a monospace font and a background; links are underlined accent text.
- Own renderer rather than `egui_commonmark`, to avoid a dependency tied to specific egui versions and to keep the palette in one place.
- Raw HTML is shown as text, never interpreted. Re-parsing happens per frame; cache the blocks per transcript item if long transcripts feel slow.

## Out of scope

Clickable links, tables, images, syntax highlighting inside code blocks, text selection across blocks, rendering the user's own messages as Markdown.

## To do

- [x] `src/markdown.rs` parser with tests
- [x] `src/ui/markdown.rs` renderer
- [x] Use it for assistant text and agent replies
- [x] Update the brief and index
- [x] Build and run the tests (builds clean, 544 pass)
- [x] Checked by eye in the running app
