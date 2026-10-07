//! The activity bar and the side panel (port of `Sidebar.tsx`). The Changes panel's file list is
//! drawn by `crate::ui::changes::file_list`.

use egui::{pos2, vec2, Align2, Margin, Rect, RichText, Sense, Stroke, StrokeKind};

use crate::sessions::{session_label, SessionInfo};

use super::changes::file_list;
use super::changes_model::ChangesModel;
use super::palette::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Session,
    Changes,
    Settings,
}

impl View {
    pub fn label(&self) -> &'static str {
        match self {
            View::Session => "Session",
            View::Changes => "Changes",
            View::Settings => "Settings",
        }
    }
}

pub struct SidebarState {
    pub view: View,
    pub expanded: bool,
}

impl Default for SidebarState {
    fn default() -> SidebarState {
        SidebarState {
            view: View::Session,
            expanded: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SidebarAction {
    /// Already applied to the state.
    SelectView(View),
    NewSession,
    OpenSession(String),
}

pub struct SidebarEnv<'a> {
    pub project: Option<&'a str>,
    pub palette: &'a Palette,
    pub sessions: &'a [SessionInfo],
    /// A session has started, so the empty-state hint no longer applies.
    pub session_started: bool,
    pub changes: &'a mut ChangesModel,
    pub changed_count: usize,
}

const BADGE_MAX: usize = 99;
const SESSION_STARTED_TEXT: &str = "Session in progress. Start a new one to clear it.";
const SESSION_TEXT: &str = "No agent session yet. Describe a task to start one.";
const SETTINGS_TEXT: &str = "Preferences stored on this machine, such as your external editor.";
const BAR_WIDTH: f32 = 48.0;

pub fn format_badge(count: usize) -> String {
    if count > BADGE_MAX {
        format!("{}+", BADGE_MAX)
    } else {
        count.to_string()
    }
}

/// What clicking the activity button for `clicked` does to the state: the active view collapses
/// the panel, any other view is shown and expands it.
pub fn apply_select(state: &mut SidebarState, clicked: View) {
    if state.view == clicked && state.expanded {
        state.expanded = false;
    } else {
        state.view = clicked;
        state.expanded = true;
    }
}

fn draw_icon(
    ui: &egui::Ui,
    view: View,
    center: egui::Pos2,
    color: egui::Color32,
    bg: egui::Color32,
) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.5, color);
    match view {
        View::Session => {
            let body = Rect::from_center_size(center + vec2(0.0, -2.0), vec2(18.0, 13.0));
            painter.rect_stroke(body, 2.0, stroke, StrokeKind::Inside);
            painter.line_segment([center + vec2(-5.0, 4.5), center + vec2(-5.0, 9.0)], stroke);
            painter.line_segment([center + vec2(-5.0, 9.0), center + vec2(0.0, 4.5)], stroke);
        }
        View::Changes => {
            let top = center + vec2(-5.0, -7.0);
            let bottom = center + vec2(-5.0, 7.0);
            let side = center + vec2(6.0, -3.0);
            painter.line_segment([top + vec2(0.0, 3.0), bottom - vec2(0.0, 3.0)], stroke);
            painter.line_segment([side + vec2(0.0, 3.0), side + vec2(0.0, 6.0)], stroke);
            painter.line_segment([side + vec2(0.0, 6.0), center + vec2(-3.0, 6.0)], stroke);
            painter.circle_stroke(top, 3.0, stroke);
            painter.circle_stroke(bottom, 3.0, stroke);
            painter.circle_stroke(side, 3.0, stroke);
        }
        View::Settings => {
            let rows: [(f32, f32); 3] = [(-6.0, 4.0), (0.0, -4.0), (6.0, 2.0)];
            for (dy, dx) in rows.iter() {
                painter.line_segment([center + vec2(-9.0, *dy), center + vec2(9.0, *dy)], stroke);
                painter.circle_filled(center + vec2(*dx, *dy), 2.5, bg);
                painter.circle_stroke(center + vec2(*dx, *dy), 2.5, stroke);
            }
        }
    }
}

fn activity_button(
    ui: &mut egui::Ui,
    rect: Rect,
    resp: &egui::Response,
    view: View,
    active: bool,
    badge: Option<usize>,
    palette: &Palette,
) {
    let color = if active || resp.hovered() {
        palette.text_strong
    } else {
        palette.text_muted
    };
    draw_icon(ui, view, rect.center(), color, palette.bg_titlebar);
    if active {
        let bar = Rect::from_min_size(rect.min, vec2(2.0, rect.height()));
        ui.painter().rect_filled(bar, 0.0, palette.text_strong);
    }
    if let Some(count) = badge {
        let text = format_badge(count);
        let width = 16.0 + 6.0 * (text.chars().count() as f32 - 1.0).max(0.0);
        let badge_rect = Rect::from_min_size(
            pos2(rect.right() - 4.0 - width, rect.top() + 4.0),
            vec2(width, 16.0),
        );
        ui.painter()
            .rect_filled(badge_rect, 8.0, palette.accent_hover);
        ui.painter().text(
            badge_rect.center(),
            Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(10.0),
            palette.text_on_accent,
        );
    }
    let _ = resp.clone().on_hover_text(view.label());
}

/// Draw the activity bar and, when expanded, the side panel. Call before the central panel.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut SidebarState,
    env: &mut SidebarEnv,
) -> Vec<SidebarAction> {
    let palette: Palette = *env.palette;
    let mut actions: Vec<SidebarAction> = Vec::new();
    let mut clicked: Option<View> = None;
    let active_view = state.view;
    let expanded = state.expanded;
    let changed_count = env.changed_count;

    egui::Panel::left("vettr-activitybar")
        .exact_size(BAR_WIDTH)
        .resizable(false)
        .frame(egui::Frame::new().fill(palette.bg_titlebar))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            for view in [View::Session, View::Changes] {
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(BAR_WIDTH, BAR_WIDTH), Sense::click());
                let active = view == active_view && expanded;
                let badge = if view == View::Changes && changed_count > 0 {
                    Some(changed_count)
                } else {
                    None
                };
                activity_button(ui, rect, &resp, view, active, badge, &palette);
                if resp.clicked() {
                    clicked = Some(view);
                }
            }
            let avail = ui.max_rect();
            let rect = Rect::from_min_size(
                pos2(avail.left(), avail.bottom() - BAR_WIDTH),
                vec2(BAR_WIDTH, BAR_WIDTH),
            );
            let resp = ui.interact(
                rect,
                egui::Id::new("vettr-activity-settings"),
                Sense::click(),
            );
            let active = active_view == View::Settings && expanded;
            activity_button(ui, rect, &resp, View::Settings, active, None, &palette);
            if resp.clicked() {
                clicked = Some(View::Settings);
            }
        });

    if let Some(view) = clicked {
        apply_select(state, view);
        actions.push(SidebarAction::SelectView(view));
    }

    if state.expanded {
        let view = state.view;
        egui::Panel::left("vettr-sidepanel")
            .exact_size(240.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(palette.bg_elevated)
                    .inner_margin(Margin::symmetric(16, 12)),
            )
            .show(ui, |ui| {
                ui.label(
                    RichText::new(view.label().to_uppercase())
                        .size(11.0)
                        .strong()
                        .color(palette.text),
                );
                ui.add_space(4.0);
                match view {
                    View::Session => {
                        let hint = if env.session_started {
                            SESSION_STARTED_TEXT
                        } else {
                            SESSION_TEXT
                        };
                        ui.label(RichText::new(hint).size(12.0).color(palette.text_muted));
                        let width = ui.available_width();
                        if ui
                            .add_sized([width, 26.0], egui::Button::new("New session"))
                            .clicked()
                        {
                            actions.push(SidebarAction::NewSession);
                        }
                        if !env.sessions.is_empty() {
                            ui.add_space(4.0);
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for session in env.sessions.iter() {
                                        let label = session_label(session);
                                        let response = ui.add(
                                            egui::Label::new(
                                                RichText::new(label.clone()).size(12.0),
                                            )
                                            .truncate()
                                            .sense(Sense::click()),
                                        );
                                        let hovered = response.hovered();
                                        let response = response
                                            .on_hover_text(label)
                                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if hovered {
                                            ui.painter().rect_stroke(
                                                response.rect.expand(2.0),
                                                2.0,
                                                Stroke::new(1.0, palette.border),
                                                StrokeKind::Outside,
                                            );
                                        }
                                        if response.clicked() {
                                            actions.push(SidebarAction::OpenSession(
                                                session.id.clone(),
                                            ));
                                        }
                                        ui.add_space(2.0);
                                    }
                                });
                        }
                    }
                    View::Changes => {
                        file_list(ui, &mut *env.changes, &palette);
                    }
                    View::Settings => {
                        ui.label(
                            RichText::new(SETTINGS_TEXT)
                                .size(12.0)
                                .color(palette.text_muted),
                        );
                    }
                }
            });
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_badge_caps_large_counts() {
        assert_eq!(format_badge(5), "5");
        assert_eq!(format_badge(99), "99");
        assert_eq!(format_badge(100), "99+");
    }

    #[test]
    fn clicking_the_active_view_collapses_the_panel() {
        let mut state = SidebarState::default();
        apply_select(&mut state, View::Session);
        assert!(!state.expanded);
        apply_select(&mut state, View::Session);
        assert!(state.expanded);
    }

    #[test]
    fn clicking_another_view_switches_and_expands() {
        let mut state = SidebarState {
            view: View::Session,
            expanded: false,
        };
        apply_select(&mut state, View::Changes);
        assert_eq!(state.view, View::Changes);
        assert!(state.expanded);
    }
}
