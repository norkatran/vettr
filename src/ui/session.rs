//! The Session view (port of `Session.tsx`): the empty-state prompt, the transcript and the
//! follow-up input. State and logic live in `agent_session.rs`; this file only draws.

use egui::{Align, Color32, CornerRadius, Frame, Label, Layout, Margin, RichText, Stroke};

use super::agent_session::{
    place_label, truncate_output, AgentSessionModel, ComposerState, TranscriptCache, MAX_OUTPUT,
};
use super::api_key_form::ApiKeyForm;
use super::notifications::Notifier;
use super::palette::{primary_button, Palette};
use super::replies::{reply_bubble, resolvable_thread, ResolvedModel};
use crate::agent::SlashCommandInfo;
use crate::backend::Backend;
use crate::comments::{ParsedReview, SentComment, Side};
use crate::profiles::ProfilesState;
use crate::readiness::{readiness_block_reason, Readiness, ReadinessReason};
use crate::replies::{reply_of, AgentReply};
use crate::session::{describe_tool, relative_path, SessionStatus, ToolStatus, TranscriptItem};

/// The widest the session column gets (the CSS `max-width: 720px`).
const MAX_WIDTH: f32 = 720.0;

/// What the Session view needs from the rest of the app.
pub struct SessionEnv<'a> {
    pub project: Option<&'a str>,
    pub readiness: &'a Readiness,
    pub profiles: &'a ProfilesState,
    pub palette: &'a Palette,
    pub resolved: &'a mut ResolvedModel,
    pub api_key_form: &'a mut ApiKeyForm,
    pub backend: &'a Backend,
    pub notifier: &'a Notifier,
}

fn muted(palette: &Palette, text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text.into())
        .size(size)
        .color(palette.text_muted)
}

/// Draw the whole Session view into `ui`.
pub fn show(ui: &mut egui::Ui, model: &mut AgentSessionModel, env: &mut SessionEnv) {
    let palette: Palette = *env.palette;
    let block: Option<String> = readiness_block_reason(env.readiness);
    let project: Option<String> = env.project.map(|p| p.to_string());
    let in_transcript = model.state().status != SessionStatus::Idle && project.is_some();

    // A centred column
    let avail = ui.available_rect_before_wrap();
    let width = (avail.width() - 48.0).min(MAX_WIDTH).max(120.0);
    let left = avail.center().x - width / 2.0;
    let top = avail.top() + 8.0;
    let bottom = (avail.bottom() - 16.0).max(top + 40.0);
    let rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + width, bottom));

    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        match project.as_deref() {
            Some(path) if in_transcript => {
                transcript_view(ui, model, env, &palette, path, block.as_deref());
            }
            _ => {
                empty_view(
                    ui,
                    model,
                    env,
                    &palette,
                    project.as_deref(),
                    block.as_deref(),
                );
            }
        }
    });
}

// ----- empty state -----

fn empty_view(
    ui: &mut egui::Ui,
    model: &mut AgentSessionModel,
    env: &mut SessionEnv<'_>,
    palette: &Palette,
    project: Option<&str>,
    block: Option<&str>,
) {
    let space_above = (ui.available_height() * 0.5 - 170.0).max(16.0);
    let no_key =
        env.readiness.reason == Some(ReadinessReason::NoKey) || env.profiles.profiles.is_empty();
    let start_error: Option<String> = model.state().start_error.clone();
    let commands: Vec<SlashCommandInfo> = model.slash_commands().to_vec();

    egui::ScrollArea::vertical()
        .id_salt("session-empty")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(space_above);
            let title = if project.is_some() {
                "What should the agent do?"
            } else {
                "Open a project to get started"
            };
            ui.label(
                RichText::new(title)
                    .size(24.0)
                    .strong()
                    .color(palette.text_strong),
            );
            ui.add_space(2.0);
            if project.is_none() {
                ui.label(muted(palette, "Use File > Open Project (Ctrl+O).", 12.0));
            }
            if project.is_some() && no_key {
                ui.label(muted(
                    palette,
                    "The agent needs a key or token before you can write a prompt. Give it a name; you can add more profiles and switch between them later in Settings.",
                    12.0,
                ));
                let ctx = ui.ctx().clone();
                env.api_key_form.show(
                    ui,
                    env.backend,
                    &ctx,
                    env.notifier,
                    env.palette,
                    env.profiles,
                    "Save",
                );
            }
            if let Some(message) = &start_error {
                ui.label(RichText::new(message).color(palette.red));
            }
            if project.is_some() && !no_key {
                let view = ComposerView {
                    id: "prompt",
                    placeholder: "Describe what you want built or changed",
                    hint: block.unwrap_or("Ctrl+Enter to start"),
                    submit_label: "Start",
                    disabled: block.is_some(),
                    rows: 6,
                    commands: &commands,
                };
                if let Some(text) = composer(ui, palette, &mut model.prompt, &view) {
                    model.start_prompt(text);
                }
            } else if project.is_none() {
                let mut empty = String::new();
                ui.add_enabled(
                    false,
                    egui::TextEdit::multiline(&mut empty)
                        .hint_text(muted(palette, "Describe what you want built or changed", 14.0))
                        .desired_width(f32::INFINITY)
                        .desired_rows(6),
                );
            }
        });
}

// ----- composer -----

struct ComposerView<'a> {
    /// Distinguishes the inputs (their egui ids).
    id: &'a str,
    placeholder: &'a str,
    hint: &'a str,
    submit_label: &'a str,
    disabled: bool,
    rows: usize,
    commands: &'a [SlashCommandInfo],
}

/// A text input with a slash command menu above it and a hint and submit button below. Returns the
/// text when the user submits (Ctrl+Enter or the button).
fn composer(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut ComposerState,
    view: &ComposerView<'_>,
) -> Option<String> {
    let id = egui::Id::new(("vettr-composer", view.id));
    let mut matches: Vec<SlashCommandInfo> = state.matches(view.commands);
    let focused = ui.memory(|m| m.has_focus(id));
    let mut moved = false;
    let mut chosen: Option<SlashCommandInfo> = None;
    let mut submit = false;

    // Keys are taken before the text input sees them
    if focused {
        let menu_open = !matches.is_empty();
        let keys = ui.input_mut(|i| {
            let none = egui::Modifiers::NONE;
            let send = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
                || i.consume_key(egui::Modifiers::CTRL, egui::Key::Enter);
            let mut down = false;
            let mut up = false;
            let mut pick = false;
            let mut escape = false;
            if menu_open {
                down = i.consume_key(none, egui::Key::ArrowDown);
                up = i.consume_key(none, egui::Key::ArrowUp);
                pick = i.consume_key(none, egui::Key::Tab) || i.consume_key(none, egui::Key::Enter);
                escape = i.consume_key(none, egui::Key::Escape);
            }
            (send, down, up, pick, escape)
        });
        let (send, down, up, pick, escape) = keys;
        let len = matches.len();
        if down {
            state.step(len, 1);
            moved = true;
        }
        if up {
            state.step(len, -1);
            moved = true;
        }
        if pick && len > 0 {
            chosen = Some(matches[state.active(len)].clone());
        }
        if escape {
            state.dismiss();
            matches = state.matches(view.commands);
        }
        submit = send;
    }
    let mut cursor_to_end = false;
    if let Some(command) = &chosen {
        state.choose(command);
        matches = state.matches(view.commands);
        cursor_to_end = true;
    }

    // The menu
    if !matches.is_empty() {
        let active = state.active(matches.len());
        let mut clicked: Option<usize> = None;
        Frame::new()
            .fill(palette.bg_elevated)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(CornerRadius::same(6))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("slash-scroll", view.id))
                    .max_height(220.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (i, command) in matches.iter().enumerate() {
                            let fill = if i == active {
                                palette.bg_input
                            } else {
                                Color32::TRANSPARENT
                            };
                            let row = Frame::new()
                                .fill(fill)
                                .inner_margin(Margin::symmetric(10, 5))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            Label::new(
                                                RichText::new(format!("/{}", command.name))
                                                    .monospace()
                                                    .color(palette.text),
                                            )
                                            .selectable(false),
                                        );
                                        if !command.argument_hint.is_empty() {
                                            ui.add(
                                                Label::new(muted(
                                                    palette,
                                                    command.argument_hint.clone(),
                                                    12.0,
                                                ))
                                                .selectable(false),
                                            );
                                        }
                                        ui.add(
                                            Label::new(muted(
                                                palette,
                                                command.description.clone(),
                                                13.0,
                                            ))
                                            .truncate()
                                            .selectable(false),
                                        );
                                    });
                                });
                            let resp = ui.interact(
                                row.response.rect,
                                egui::Id::new(("slash-row", view.id, i)),
                                egui::Sense::click(),
                            );
                            if resp.clicked() {
                                clicked = Some(i);
                            }
                            if i == active && moved {
                                resp.scroll_to_me(None);
                            }
                            let _ = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                        }
                    });
            });
        if let Some(index) = clicked {
            state.choose(&matches[index]);
            cursor_to_end = true;
        }
        ui.add_space(4.0);
    }
    let menu_open = !state.matches(view.commands).is_empty();

    if cursor_to_end {
        if let Some(mut edit_state) = egui::TextEdit::load_state(ui.ctx(), id) {
            let end = state.text.chars().count();
            edit_state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(end),
                )));
            edit_state.store(ui.ctx(), id);
        }
    }

    let mut edit = egui::TextEdit::multiline(&mut state.text)
        .id(id)
        .hint_text(muted(palette, view.placeholder, 14.0))
        .desired_width(f32::INFINITY)
        .desired_rows(view.rows)
        .margin(egui::Margin::symmetric(16, 14))
        .text_color(palette.text_strong);
    if menu_open {
        // Tab and Escape act on the menu, not on the focus
        edit = edit.event_filter(egui::EventFilter {
            tab: true,
            horizontal_arrows: true,
            vertical_arrows: true,
            escape: true,
        });
    }
    let response = ui.add(edit);
    if state.focus_pending || cursor_to_end {
        response.request_focus();
        state.focus_pending = false;
    }
    if response.changed() {
        state.edited();
    }

    ui.add_space(2.0);
    let total = ui.available_width();
    let can_send = !view.disabled && !state.text.trim().is_empty();
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2((total - 110.0).max(40.0), 24.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.add(Label::new(muted(palette, view.hint, 12.0)).truncate());
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(can_send, primary_button(palette, view.submit_label))
                .clicked()
            {
                submit = true;
            }
        });
    });

    if submit {
        state.take_submit(view.disabled)
    } else {
        None
    }
}

// ----- transcript -----

fn transcript_view(
    ui: &mut egui::Ui,
    model: &mut AgentSessionModel,
    env: &mut SessionEnv<'_>,
    palette: &Palette,
    project: &str,
    block: Option<&str>,
) {
    let status = model.state().status;
    let interrupting = model.state().interrupting;
    let ended_without_session =
        status == SessionStatus::Ended && model.state().session_id.is_none();
    let cache = model.cache();
    let commands: Vec<SlashCommandInfo> = model.slash_commands().to_vec();
    let loads = model.loads();
    if status == SessionStatus::Running {
        // The follow-up input is focused again once the turn ends
        model.follow_up.focus_pending = true;
    }
    let mut stop = false;
    let mut new_session = false;
    let mut send_text: Option<String> = None;

    egui::Panel::bottom(egui::Id::new("session-composer"))
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::NONE)
        .show(ui, |ui| {
            ui.add_space(8.0);
            if ended_without_session {
                Frame::new()
                    .fill(palette.bg_elevated)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label("The session has ended.");
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("New session").clicked() {
                                    new_session = true;
                                }
                            });
                        });
                    });
            } else if status == SessionStatus::Running {
                ui.horizontal(|ui| {
                    let hint = if interrupting {
                        "Stopping..."
                    } else {
                        "The agent is working"
                    };
                    ui.label(muted(palette, hint, 12.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_enabled(!interrupting, egui::Button::new("Stop"))
                            .clicked()
                        {
                            stop = true;
                        }
                    });
                });
            } else {
                let placeholder = if status == SessionStatus::Ended {
                    "The agent exited. Send a message to resume this session"
                } else {
                    "Send a follow-up"
                };
                let view = ComposerView {
                    id: "follow-up",
                    placeholder,
                    hint: block.unwrap_or("Ctrl+Enter to send"),
                    submit_label: "Send",
                    disabled: block.is_some(),
                    rows: 3,
                    commands: &commands,
                };
                send_text = composer(ui, palette, &mut model.follow_up, &view);
            }
        });

    egui::ScrollArea::vertical()
        .id_salt(("session-transcript", loads))
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.add_space(4.0);
            for (index, item) in model.state().items.iter().enumerate() {
                show_item(
                    ui,
                    palette,
                    index,
                    item,
                    project,
                    &cache,
                    &mut *env.resolved,
                );
            }
            if status == SessionStatus::Running {
                ui.label(muted(palette, "Working...", 12.0));
            }
            ui.add_space(4.0);
        });

    if new_session {
        model.new_session();
    }
    if stop {
        model.interrupt();
    }
    if let Some(text) = send_text {
        model.send_follow_up(text);
    }
}

fn show_item(
    ui: &mut egui::Ui,
    palette: &Palette,
    index: usize,
    item: &TranscriptItem,
    project: &str,
    cache: &TranscriptCache,
    resolved: &mut ResolvedModel,
) {
    match item {
        TranscriptItem::User { text } => match cache.reviews.get(&index) {
            Some(review) => review_message(ui, palette, review, cache, resolved),
            None => user_bubble(ui, palette, text),
        },
        TranscriptItem::Text { text } => {
            Frame::new()
                .inner_margin(Margin::symmetric(14, 6))
                .show(ui, |ui| {
                    super::markdown::show(ui, palette, text);
                });
        }
        TranscriptItem::Edit { path } => {
            ui.label(
                RichText::new(format!("Edited {}", relative_path(project, path)))
                    .monospace()
                    .size(12.0)
                    .color(palette.text_muted),
            );
        }
        TranscriptItem::Error { message } => {
            Frame::new()
                .stroke(Stroke::new(1.0, palette.red))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add(Label::new(RichText::new(message).color(palette.red)).wrap());
                });
        }
        TranscriptItem::Notice { text } => {
            ui.label(muted(palette, text.clone(), 12.0));
        }
        TranscriptItem::Tool {
            name,
            input,
            status,
            output,
            ..
        } => match reply_of(item) {
            Some(reply) => {
                let place = cache.places.get(&reply.comment_id).map(|s| s.as_str());
                reply_bubble(ui, palette, &reply, place);
            }
            None => tool_row(ui, palette, index, name, input, *status, output),
        },
    }
}

fn user_bubble(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    // The bubble hugs the right edge, but its text is left-aligned so pasted Markdown and fenced
    // code stay readable (the outer Max layout would otherwise be inherited by the label).
    ui.with_layout(Layout::top_down(Align::Max), |ui| {
        ui.set_max_width(ui.available_width() * 0.85);
        Frame::new()
            .fill(palette.accent)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.add(Label::new(RichText::new(text).color(palette.text_on_accent)).wrap());
                });
            });
    });
}

fn code_block(ui: &mut egui::Ui, palette: &Palette, index: usize, which: u8, text: &str) {
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt(("tool-pre", index, which))
        .max_height(260.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
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
}

fn tool_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    index: usize,
    name: &str,
    input: &serde_json::Value,
    status: ToolStatus,
    output: &str,
) {
    let (icon, color) = match status {
        ToolStatus::Running => ("...", palette.text_muted),
        ToolStatus::Done => ("ok", palette.green),
        ToolStatus::Error => ("x", palette.red),
        ToolStatus::Stopped => ("-", palette.text_muted),
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(
        icon,
        0.0,
        egui::text::TextFormat::simple(egui::FontId::monospace(12.0), color),
    );
    job.append(
        name,
        8.0,
        egui::text::TextFormat::simple(egui::FontId::proportional(12.0), palette.text_strong),
    );
    job.append(
        &describe_tool(input),
        8.0,
        egui::text::TextFormat::simple(egui::FontId::monospace(12.0), palette.text_muted),
    );
    Frame::new()
        .fill(palette.bg_elevated)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::CollapsingHeader::new(job)
                .id_salt(("tool-row", index))
                .default_open(false)
                .show(ui, |ui| {
                    let pretty = serde_json::to_string_pretty(input).unwrap_or_default();
                    code_block(ui, palette, index, 0, &pretty);
                    if !output.is_empty() {
                        code_block(ui, palette, index, 1, &truncate_output(output, MAX_OUTPUT));
                    }
                });
        });
}

/// A review round the user sent, rebuilt from the message so it survives reloads and resumes.
fn review_message(
    ui: &mut egui::Ui,
    palette: &Palette,
    review: &ParsedReview,
    cache: &TranscriptCache,
    resolved: &mut ResolvedModel,
) {
    let total = ui.available_width();
    ui.with_layout(Layout::top_down(Align::Max), |ui| {
        ui.set_width(total * 0.85);
        ui.vertical(|ui| {
            let n = review.comments.len();
            ui.label(muted(
                palette,
                format!(
                    "Review, round {}: {} {}",
                    review.round,
                    n,
                    if n == 1 { "comment" } else { "comments" }
                ),
                11.0,
            ));
            for comment in review.comments.iter() {
                let place = place_label(comment);
                let replies: &[AgentReply] = match cache.threads.get(&comment.id) {
                    Some(list) => list.as_slice(),
                    None => &[],
                };
                resolvable_thread(ui, palette, &mut *resolved, &comment.id, &place, |ui| {
                    comment_card(ui, palette, comment, &place, replies);
                });
            }
        });
    });
}

fn comment_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    comment: &SentComment,
    place: &str,
    replies: &[AgentReply],
) {
    let border = if comment.outdated {
        palette.amber
    } else {
        palette.border
    };
    Frame::new()
        .fill(palette.bg_input)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let mut meta = place.to_string();
                if matches!(comment.side, Side::Old) {
                    meta.push_str(" (removed code)");
                }
                ui.label(muted(palette, meta, 11.0));
                if comment.outdated {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new("outdated").size(11.0).color(palette.amber));
                    });
                }
            });
            if !comment.snapshot.is_empty() {
                Frame::new()
                    .fill(palette.bg_elevated)
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.add(
                            Label::new(
                                RichText::new(comment.snapshot.join("\n"))
                                    .monospace()
                                    .size(12.0)
                                    .color(palette.text),
                            )
                            .wrap(),
                        );
                    });
            }
            ui.add(Label::new(RichText::new(&comment.text).color(palette.text_strong)).wrap());
            for reply in replies.iter() {
                reply_bubble(ui, palette, reply, None);
            }
        });
}
