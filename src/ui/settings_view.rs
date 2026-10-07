//! The Settings view (port of `SettingsView.tsx`): the saved profiles and the external editor.

use std::collections::HashMap;

use egui::RichText;

use crate::backend::Backend;
use crate::profiles::{ProfileInfo, ProfilesState};
use crate::settings::{match_preset, Settings, EDITOR_PRESETS};
use crate::task::Task;

use super::api_key_form::{ApiKeyForm, FormEvent};
use super::notifications::Notifier;
use super::palette::Palette;

pub struct SettingsEnv<'a> {
    pub palette: &'a Palette,
    pub backend: &'a Backend,
    pub notifier: &'a Notifier,
    pub ctx: &'a egui::Context,
    pub profiles: &'a ProfilesState,
    /// The form that adds a profile (also used by the first-run flow elsewhere).
    pub api_key_form: &'a mut ApiKeyForm,
}

/// What the editor dropdown shows as selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorChoice {
    NotSet,
    /// The index into `EDITOR_PRESETS`.
    Preset(usize),
    Custom,
}

/// Picking "Custom" with an empty command has no value to infer it from, hence the `custom` flag.
pub fn selected_choice(custom: bool, command: &str) -> EditorChoice {
    if custom {
        return EditorChoice::Custom;
    }
    match EDITOR_PRESETS.iter().position(|p| p.command == command) {
        Some(index) => EditorChoice::Preset(index),
        None => EditorChoice::NotSet,
    }
}

/// Whether a loaded command should start in custom mode.
pub fn starts_custom(command: &str) -> bool {
    !command.is_empty() && match_preset(command).is_none()
}

/// The command after choosing `choice` (custom keeps what was typed).
pub fn command_after_choice(choice: EditorChoice, current: &str) -> String {
    match choice {
        EditorChoice::NotSet => String::new(),
        EditorChoice::Preset(index) => EDITOR_PRESETS[index].command.to_string(),
        EditorChoice::Custom => current.to_string(),
    }
}

fn choice_label(choice: EditorChoice) -> String {
    match choice {
        EditorChoice::NotSet => "Not set".to_string(),
        EditorChoice::Preset(index) => EDITOR_PRESETS[index].label.to_string(),
        EditorChoice::Custom => "Custom command".to_string(),
    }
}

enum RowAction {
    Use(String),
    Edit(String, String),
    AskRemove(String),
    ConfirmRemove(String),
    CancelRemove,
}

pub struct SettingsViewState {
    load_task: Option<Task<Settings>>,
    settings: Option<Settings>,
    custom: bool,
    started: bool,
    dirty: bool,
    save_task: Option<Task<Settings>>,
    adding: bool,
    editing: HashMap<String, ApiKeyForm>,
    removing: Option<String>,
    row_errors: HashMap<String, String>,
    /// The profile a use/remove call is running for.
    op_task: Option<(String, Task<Result<(), String>>)>,
}

impl Default for SettingsViewState {
    fn default() -> SettingsViewState {
        SettingsViewState::new()
    }
}

fn take_result<T: Send + 'static>(task: &mut Task<T>) -> (Option<T>, bool) {
    let mut result = task.poll();
    if result.is_none() && !task.is_running() {
        result = task.poll();
    }
    let keep = result.is_none() && task.is_running();
    (result, keep)
}

impl SettingsViewState {
    pub fn new() -> SettingsViewState {
        SettingsViewState {
            load_task: None,
            settings: None,
            custom: false,
            started: false,
            dirty: false,
            save_task: None,
            adding: false,
            editing: HashMap::new(),
            removing: None,
            row_errors: HashMap::new(),
            op_task: None,
        }
    }

    fn pump(&mut self, env: &SettingsEnv) {
        if !self.started {
            self.started = true;
            let backend = env.backend.clone();
            self.load_task = Some(Task::spawn(env.ctx, move || backend.settings()));
        }
        if let Some(mut task) = self.load_task.take() {
            let (result, keep) = take_result(&mut task);
            if let Some(settings) = result {
                self.custom = starts_custom(&settings.editor_command);
                self.settings = Some(settings);
            } else if keep {
                self.load_task = Some(task);
            } else {
                // The load failed: start from the defaults rather than showing nothing
                self.settings = Some(Settings::default());
            }
        }
        // Saves run one at a time so an older value can never overwrite a newer one
        if let Some(mut task) = self.save_task.take() {
            let (_, keep) = take_result(&mut task);
            if keep {
                self.save_task = Some(task);
            }
        }
        if self.save_task.is_none() && self.dirty {
            if let Some(settings) = self.settings.clone() {
                self.dirty = false;
                let backend = env.backend.clone();
                self.save_task = Some(Task::spawn(env.ctx, move || {
                    backend.set_settings(&settings)
                }));
            }
        }
        if let Some((id, mut task)) = self.op_task.take() {
            let (result, keep) = take_result(&mut task);
            match result {
                Some(Ok(())) => {
                    self.row_errors.remove(&id);
                }
                Some(Err(message)) => {
                    self.row_errors.insert(id, message);
                }
                None => {
                    if keep {
                        self.op_task = Some((id, task));
                    }
                }
            }
        }
    }

    fn start_op(&mut self, env: &SettingsEnv, id: &str, remove: bool) {
        if self.op_task.is_some() {
            return;
        }
        self.row_errors.remove(id);
        let backend = env.backend.clone();
        let target = id.to_string();
        let task: Task<Result<(), String>> = Task::spawn(env.ctx, move || {
            if remove {
                backend.remove_profile(&target)
            } else {
                backend.set_active_profile(&target)
            }
        });
        self.op_task = Some((id.to_string(), task));
    }
}

fn hint(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(palette.text_muted));
}

fn profile_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    profile: &ProfileInfo,
    active: bool,
    confirming: bool,
    busy: bool,
    error: Option<&String>,
    actions: &mut Vec<RowAction>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(profile.name.as_str())
                .strong()
                .color(palette.text_strong),
        );
        if active {
            ui.label(RichText::new("In use").size(11.0).color(palette.green));
        }
        if !active
            && ui
                .add_enabled(!busy, egui::Button::new("Use"))
                .on_hover_text("Use this profile in this window; restarts the agent")
                .clicked()
        {
            actions.push(RowAction::Use(profile.id.clone()));
        }
        if ui.button("Edit").clicked() {
            actions.push(RowAction::Edit(profile.id.clone(), profile.name.clone()));
        }
        if confirming {
            if ui
                .add_enabled(!busy, egui::Button::new("Confirm remove"))
                .clicked()
            {
                actions.push(RowAction::ConfirmRemove(profile.id.clone()));
            }
            if ui.button("Cancel").clicked() {
                actions.push(RowAction::CancelRemove);
            }
        } else if ui.button("Remove").clicked() {
            actions.push(RowAction::AskRemove(profile.id.clone()));
        }
        if busy {
            ui.spinner();
        }
    });
    if let Some(message) = error {
        ui.label(RichText::new(message.as_str()).color(palette.red));
    }
}

fn profiles_section(ui: &mut egui::Ui, state: &mut SettingsViewState, env: &mut SettingsEnv) {
    let palette: Palette = *env.palette;
    let list: Vec<ProfileInfo> = env.profiles.profiles.clone();
    let active_id: Option<String> = env.profiles.active_id.clone();
    ui.label(
        RichText::new("Claude profiles")
            .strong()
            .color(palette.text_strong),
    );
    hint(
        ui,
        &palette,
        if list.is_empty() {
            "No profile is saved, so the agent cannot start."
        } else {
            "Each profile is a named API key or OAuth token. The one in use applies to this app instance only; other open instances keep theirs."
        },
    );
    ui.add_space(4.0);
    let busy = state.op_task.is_some();
    let mut row_actions: Vec<RowAction> = Vec::new();
    for profile in list.iter() {
        ui.push_id(profile.id.as_str(), |ui| {
            let mut finished = false;
            if let Some(form) = state.editing.get_mut(&profile.id) {
                let event = form.show_form(
                    ui,
                    env.backend,
                    env.ctx,
                    env.notifier,
                    env.palette,
                    env.profiles,
                    "Save",
                    true,
                );
                finished = event != FormEvent::None;
            } else {
                let confirming = state.removing.as_deref() == Some(profile.id.as_str());
                let is_active = active_id.as_deref() == Some(profile.id.as_str());
                let error = state.row_errors.get(&profile.id);
                profile_row(
                    ui,
                    &palette,
                    profile,
                    is_active,
                    confirming,
                    busy,
                    error,
                    &mut row_actions,
                );
            }
            if finished {
                state.editing.remove(&profile.id);
            }
            ui.add_space(6.0);
        });
    }
    for action in row_actions {
        match action {
            RowAction::Use(id) => state.start_op(env, &id, false),
            RowAction::Edit(id, name) => {
                state
                    .editing
                    .insert(id.clone(), ApiKeyForm::editing(&id, &name));
            }
            RowAction::AskRemove(id) => state.removing = Some(id),
            RowAction::ConfirmRemove(id) => {
                state.removing = None;
                state.start_op(env, &id, true);
            }
            RowAction::CancelRemove => state.removing = None,
        }
    }
    if list.is_empty() || state.adding {
        let event = env.api_key_form.show_form(
            ui,
            env.backend,
            env.ctx,
            env.notifier,
            env.palette,
            env.profiles,
            "Add profile",
            !list.is_empty(),
        );
        if event != FormEvent::None {
            state.adding = false;
        }
    } else if ui.button("Add profile").clicked() {
        state.adding = true;
    }
}

fn editor_section(ui: &mut egui::Ui, state: &mut SettingsViewState, palette: &Palette) {
    let current: String = match &state.settings {
        Some(settings) => settings.editor_command.clone(),
        None => return,
    };
    let selected = selected_choice(state.custom, &current);
    ui.label(
        RichText::new("External editor")
            .strong()
            .color(palette.text_strong),
    );
    let mut chosen: Option<EditorChoice> = None;
    egui::ComboBox::from_id_salt("editor-preset")
        .selected_text(choice_label(selected))
        .width(220.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(selected == EditorChoice::NotSet, "Not set")
                .clicked()
            {
                chosen = Some(EditorChoice::NotSet);
            }
            for (index, preset) in EDITOR_PRESETS.iter().enumerate() {
                if ui
                    .selectable_label(selected == EditorChoice::Preset(index), preset.label)
                    .clicked()
                {
                    chosen = Some(EditorChoice::Preset(index));
                }
            }
            if ui
                .selectable_label(selected == EditorChoice::Custom, "Custom command")
                .clicked()
            {
                chosen = Some(EditorChoice::Custom);
            }
        });
    if let Some(choice) = chosen {
        state.custom = choice == EditorChoice::Custom;
        let next = command_after_choice(choice, &current);
        if next != current {
            if let Some(settings) = state.settings.as_mut() {
                settings.editor_command = next;
            }
            state.dirty = true;
        }
    }
    if selected == EditorChoice::Custom {
        let mut command = current.clone();
        let response = ui.add(
            egui::TextEdit::singleline(&mut command)
                .hint_text("my-editor --goto {file}:{line}")
                .desired_width(f32::INFINITY),
        );
        if response.changed() {
            if let Some(settings) = state.settings.as_mut() {
                settings.editor_command = command;
            }
            state.dirty = true;
        }
    }
    hint(
        ui,
        palette,
        "Used by \"Open in editor\". {file}, {line} and {project} are replaced with the file path, line number and project folder.",
    );
    if current.is_empty() && selected != EditorChoice::Custom {
        hint(ui, palette, "No editor set yet.");
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut SettingsViewState, env: &mut SettingsEnv) {
    state.pump(env);
    let palette: Palette = *env.palette;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(24, 16))
                .show(ui, |ui| {
                    ui.set_max_width(640.0);
                    ui.label(
                        RichText::new("Settings")
                            .size(20.0)
                            .strong()
                            .color(palette.text_strong),
                    );
                    ui.add_space(12.0);
                    profiles_section(ui, state, env);
                    ui.add_space(16.0);
                    if state.settings.is_some() {
                        editor_section(ui, state, &palette);
                    } else {
                        ui.spinner();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dropdown_shows_the_matching_preset() {
        let first = EDITOR_PRESETS[0].command;
        assert_eq!(selected_choice(false, first), EditorChoice::Preset(0));
        assert_eq!(selected_choice(false, ""), EditorChoice::NotSet);
        assert_eq!(selected_choice(false, "nano {file}"), EditorChoice::NotSet);
        assert_eq!(selected_choice(true, first), EditorChoice::Custom);
    }

    #[test]
    fn a_custom_command_starts_in_custom_mode() {
        assert!(starts_custom("nano {file}"));
        assert!(!starts_custom(""));
        assert!(!starts_custom(EDITOR_PRESETS[1].command));
    }

    #[test]
    fn choosing_sets_the_command() {
        assert_eq!(command_after_choice(EditorChoice::NotSet, "x"), "");
        assert_eq!(
            command_after_choice(EditorChoice::Preset(2), "x"),
            EDITOR_PRESETS[2].command
        );
        assert_eq!(command_after_choice(EditorChoice::Custom, "x"), "x");
    }
}
