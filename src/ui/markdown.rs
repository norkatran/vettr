//! Draws the block tree from `crate::markdown` (design 0009). egui has no bold font, so bold text
//! uses the strong text colour instead.

use egui::text::{LayoutJob, TextFormat};
use egui::{CornerRadius, FontId, Frame, Label, Margin, RichText, Stroke};

use super::palette::Palette;
use crate::markdown::{parse, Alignment, Block, Span};

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
        Block::Table { aligns, rows } => {
            table_ui(ui, palette, aligns, rows);
            ui.add_space(4.0);
        }
        Block::Rule => {
            ui.separator();
        }
    }
}

/// Draw a table with explicit column widths: each column gets its natural (unwrapped) width when
/// everything fits, otherwise narrow columns keep theirs and wide ones share the rest in
/// proportion, so cells wrap instead of collapsing to slivers.
fn table_ui(ui: &mut egui::Ui, palette: &Palette, aligns: &[Alignment], rows: &[Vec<Vec<Span>>]) {
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return;
    }
    let pad = 8.0;
    let gap = 12.0;
    let styled = |r: usize, cell: &[Span]| {
        let mut spans = cell.to_vec();
        if r == 0 {
            spans.iter_mut().for_each(|s| s.bold = true);
        }
        spans
    };
    let mut natural = vec![0.0f32; cols];
    for (r, cells) in rows.iter().enumerate() {
        for (c, cell) in cells.iter().enumerate() {
            let galley = ui
                .painter()
                .layout_job(job(palette, &styled(r, cell), BODY, false));
            natural[c] = natural[c].max(galley.size().x.ceil() + 1.0);
        }
    }
    let avail = (ui.available_width() - 2.0 * pad - gap * (cols as f32 - 1.0) - 4.0)
        .max(40.0 * cols as f32);
    let widths = fit_columns(&natural, avail);

    Frame::new()
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(4))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (r, cells) in rows.iter().enumerate() {
                let fill = if r == 0 || r % 2 == 0 {
                    palette.bg_input
                } else {
                    egui::Color32::TRANSPARENT
                };
                Frame::new()
                    .fill(fill)
                    .inner_margin(Margin::symmetric(pad as i8, 4))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        ui.horizontal_top(|ui| {
                            for (c, w) in widths.iter().enumerate() {
                                let empty = Vec::new();
                                let cell = cells.get(c).unwrap_or(&empty);
                                let halign = match aligns.get(c) {
                                    Some(Alignment::Center) => egui::Align::Center,
                                    Some(Alignment::Right) => egui::Align::Max,
                                    _ => egui::Align::Min,
                                };
                                let mut j = job(palette, &styled(r, cell), BODY, false);
                                j.halign = halign;
                                ui.allocate_ui_with_layout(
                                    egui::vec2(*w, 0.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_width(*w);
                                        ui.add(Label::new(j).wrap());
                                    },
                                );
                            }
                        });
                    });
            }
        });
}

/// Shrink column widths to fit `avail`: columns at or under their fair share keep their natural
/// width, the rest split what is left in proportion to their natural width.
fn fit_columns(natural: &[f32], avail: f32) -> Vec<f32> {
    if natural.iter().sum::<f32>() <= avail {
        return natural.to_vec();
    }
    let n = natural.len();
    let mut widths = natural.to_vec();
    let mut fixed = vec![false; n];
    loop {
        let used: f32 = (0..n).filter(|&i| fixed[i]).map(|i| widths[i]).sum();
        let free = avail - used;
        let flex_total: f32 = (0..n).filter(|&i| !fixed[i]).map(|i| natural[i]).sum();
        let flex_count = fixed.iter().filter(|f| !**f).count();
        if flex_count == 0 {
            break;
        }
        let share = free / flex_count as f32;
        // Fix any column that needs less than an equal share; repeat with the remainder.
        if let Some(i) = (0..n).find(|&i| !fixed[i] && natural[i] <= share) {
            fixed[i] = true;
            widths[i] = natural[i];
            continue;
        }
        for i in 0..n {
            if !fixed[i] {
                widths[i] = (free * natural[i] / flex_total).max(30.0);
            }
        }
        break;
    }
    widths
}

#[cfg(test)]
mod tests {
    use super::fit_columns;

    #[test]
    fn narrow_columns_keep_width_and_wide_ones_share() {
        let w = fit_columns(&[40.0, 400.0, 400.0], 500.0);
        assert_eq!(w[0], 40.0);
        assert!((w[1] - 230.0).abs() < 0.01 && (w[2] - 230.0).abs() < 0.01);
        assert_eq!(fit_columns(&[10.0, 20.0], 100.0), vec![10.0, 20.0]);
    }
}
