//! Draws the block tree from `crate::markdown` (design 0009). egui has no bold font, so bold text
//! uses the strong text colour instead.

use egui::text::{LayoutJob, TextFormat};
use egui::{CornerRadius, FontId, Frame, Label, Margin, RichText, Stroke};

use super::palette::Palette;
use crate::markdown::{parse, Block, Span};

const BODY: f32 = 14.0;

/// Render `source` as Markdown in the current layout.
pub fn show(ui: &mut egui::Ui, palette: &Palette, source: &str) {
    let blocks = parse(source);
    blocks_ui(ui, palette, &blocks);
}

fn blocks_ui(ui: &mut egui::Ui, palette: &Palette, blocks: &[Block]) {
    for block in blocks {
        block_ui(ui, palette, block);
    }
}

fn job(palette: &Palette, spans: &[Span], size: f32, heading: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    for span in spans {
        let font = if span.code {
            FontId::monospace(size - 1.0)
        } else {
            FontId::proportional(size)
        };
        let color = if span.link.is_some() {
            palette.accent
        } else if span.bold || heading {
            palette.text_strong
        } else {
            palette.text
        };
        let mut format = TextFormat {
            font_id: font,
            color,
            italics: span.italic,
            ..Default::default()
        };
        if span.code {
            format.background = palette.bg_input;
        }
        if span.strike {
            format.strikethrough = Stroke::new(1.0, color);
        }
        if span.link.is_some() {
            format.underline = Stroke::new(1.0, color);
        }
        job.append(&span.text, 0.0, format);
    }
    job
}

fn block_ui(ui: &mut egui::Ui, palette: &Palette, block: &Block) {
    match block {
        Block::Paragraph(spans) => {
            ui.add(Label::new(job(palette, spans, BODY, false)).wrap());
            ui.add_space(4.0);
        }
        Block::Heading(level, spans) => {
            let size = match level {
                1 => 20.0,
                2 => 18.0,
                3 => 16.0,
                _ => BODY,
            };
            ui.add_space(4.0);
            ui.add(Label::new(job(palette, spans, size, true)).wrap());
            ui.add_space(4.0);
        }
        Block::Code { text, .. } => {
            Frame::new()
                .fill(palette.bg_input)
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add(
                        Label::new(
                            RichText::new(text)
                                .monospace()
                                .size(12.0)
                                .color(palette.text),
                        )
                        .wrap(),
                    );
                });
            ui.add_space(4.0);
        }
        Block::Quote(inner) => {
            Frame::new()
                .stroke(Stroke::new(1.0, palette.border))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    blocks_ui(ui, palette, inner);
                });
            ui.add_space(4.0);
        }
        Block::List { start, items } => {
            for (i, item) in items.iter().enumerate() {
                let marker = match start {
                    Some(n) => format!("{}.", n + i as u64),
                    None => "\u{2022}".to_string(),
                };
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new(marker).color(palette.text_muted));
                    ui.vertical(|ui| blocks_ui(ui, palette, item));
                });
            }
        }
        Block::Rule => {
            ui.separator();
        }
    }
}
