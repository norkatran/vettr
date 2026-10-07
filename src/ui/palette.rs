//! Colours for the light and dark themes (the CSS variables of the old `styles.css`) and the
//! function that applies them to egui's `Visuals`. Every view takes its colours from here.

use egui::{Color32, Stroke};

use crate::highlight::TokenKind;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub bg_elevated: Color32,
    pub bg_titlebar: Color32,
    pub bg_input: Color32,
    pub border: Color32,
    pub border_focus: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    pub text_muted: Color32,
    pub text_on_accent: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    /// Added lines and counts (`#3fb950`).
    pub green: Color32,
    /// Deleted lines, errors (`#f85149`).
    pub red: Color32,
    /// Warnings, outdated markers (`#d29922`).
    pub amber: Color32,
    /// Row background of added diff lines.
    pub add_bg: Color32,
    /// Row background of deleted diff lines.
    pub del_bg: Color32,
    /// Background of selected (commented) line ranges.
    pub select_bg: Color32,
    hl_comment: Color32,
    hl_keyword: Color32,
    hl_string: Color32,
    hl_number: Color32,
    hl_title: Color32,
    hl_type: Color32,
    hl_attr: Color32,
    hl_meta: Color32,
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

impl Palette {
    pub fn new(theme: Theme) -> Palette {
        match theme {
            Theme::Dark => Palette {
                dark: true,
                bg: rgb(0x1e1e1e),
                bg_elevated: rgb(0x252526),
                bg_titlebar: rgb(0x181818),
                bg_input: rgb(0x2b2b2c),
                border: rgb(0x3c3c3c),
                border_focus: rgb(0x0e7ad1),
                text: rgb(0xcccccc),
                text_strong: rgb(0xffffff),
                text_muted: rgb(0x8b8b8b),
                text_on_accent: rgb(0xffffff),
                accent: rgb(0x0e639c),
                accent_hover: rgb(0x1177bb),
                green: rgb(0x3fb950),
                red: rgb(0xf85149),
                amber: rgb(0xd29922),
                add_bg: Color32::from_rgba_unmultiplied(63, 185, 80, 38),
                del_bg: Color32::from_rgba_unmultiplied(248, 81, 73, 38),
                select_bg: Color32::from_rgba_unmultiplied(88, 166, 255, 90),
                hl_comment: rgb(0x6a9955),
                hl_keyword: rgb(0xc586c0),
                hl_string: rgb(0xce9178),
                hl_number: rgb(0xb5cea8),
                hl_title: rgb(0xdcdcaa),
                hl_type: rgb(0x4ec9b0),
                hl_attr: rgb(0x9cdcfe),
                hl_meta: rgb(0x569cd6),
            },
            Theme::Light => Palette {
                dark: false,
                bg: rgb(0xffffff),
                bg_elevated: rgb(0xf3f3f3),
                bg_titlebar: rgb(0xe8e8e8),
                bg_input: rgb(0xffffff),
                border: rgb(0xd0d0d0),
                border_focus: rgb(0x0e7ad1),
                text: rgb(0x3b3b3b),
                text_strong: rgb(0x000000),
                text_muted: rgb(0x6e6e6e),
                text_on_accent: rgb(0xffffff),
                accent: rgb(0x0e639c),
                accent_hover: rgb(0x1177bb),
                green: rgb(0x3fb950),
                red: rgb(0xf85149),
                amber: rgb(0xd29922),
                add_bg: Color32::from_rgba_unmultiplied(63, 185, 80, 38),
                del_bg: Color32::from_rgba_unmultiplied(248, 81, 73, 38),
                select_bg: Color32::from_rgba_unmultiplied(88, 166, 255, 90),
                hl_comment: rgb(0x008000),
                hl_keyword: rgb(0xaf00db),
                hl_string: rgb(0xa31515),
                hl_number: rgb(0x098658),
                hl_title: rgb(0x795e26),
                hl_type: rgb(0x267f99),
                hl_attr: rgb(0x001080),
                hl_meta: rgb(0x0000ff),
            },
        }
    }

    /// Colour of a syntax token in a diff line.
    pub fn token_color(&self, kind: TokenKind) -> Color32 {
        match kind {
            TokenKind::Plain | TokenKind::Operator | TokenKind::Punctuation => self.text,
            TokenKind::Comment => self.hl_comment,
            TokenKind::Keyword => self.hl_keyword,
            TokenKind::String => self.hl_string,
            TokenKind::Number | TokenKind::Constant => self.hl_number,
            TokenKind::Function => self.hl_title,
            TokenKind::Type => self.hl_type,
            TokenKind::Attribute => self.hl_attr,
            TokenKind::Tag => self.hl_meta,
        }
    }
}

/// Apply the palette to egui's global style (colours, rounding, sizes close to the CSS).
pub fn apply(ctx: &egui::Context, palette: &Palette) {
    let mut visuals = if palette.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.override_text_color = Some(palette.text);
    visuals.panel_fill = palette.bg;
    visuals.window_fill = palette.bg_elevated;
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.extreme_bg_color = palette.bg_input;
    visuals.faint_bg_color = palette.bg_elevated;
    visuals.code_bg_color = palette.bg_elevated;
    visuals.hyperlink_color = palette.border_focus;
    visuals.error_fg_color = palette.red;
    visuals.warn_fg_color = palette.amber;
    visuals.selection.bg_fill = palette.accent;
    visuals.selection.stroke = Stroke::new(1.0, palette.text_on_accent);

    visuals.widgets.noninteractive.bg_fill = palette.bg;
    visuals.widgets.noninteractive.weak_bg_fill = palette.bg;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);

    visuals.widgets.inactive.bg_fill = palette.bg_elevated;
    visuals.widgets.inactive.weak_bg_fill = palette.bg_elevated;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);

    visuals.widgets.hovered.bg_fill = palette.accent_hover;
    visuals.widgets.hovered.weak_bg_fill = palette.bg_input;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.border_focus);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text_strong);

    visuals.widgets.active.bg_fill = palette.accent;
    visuals.widgets.active.weak_bg_fill = palette.accent;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.border_focus);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, palette.text_on_accent);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    ctx.set_global_style(style);
}

/// A flat accent-coloured button, as used for primary actions in the old UI.
pub fn primary_button(palette: &Palette, text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.into()).color(palette.text_on_accent))
        .fill(palette.accent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colours_match_the_stylesheet() {
        assert_eq!(rgb(0x1e1e1e), Color32::from_rgb(30, 30, 30));
        assert_eq!(
            Palette::new(Theme::Dark).accent,
            Color32::from_rgb(14, 99, 156)
        );
        assert!(!Palette::new(Theme::Light).dark);
    }
}
