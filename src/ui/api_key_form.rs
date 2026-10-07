//! Entry for a profile (port of `ApiKeyForm.tsx`): a name and an Anthropic API key or Claude
//! OAuth token. The credential field is masked (the TS showed it as text) and is cleared as soon
//! as it is saved: a saved key is never displayed again. When editing, the key may be left blank
//! to keep the saved one.

use std::sync::atomic::{AtomicU64, Ordering};

use egui::RichText;

use crate::backend::Backend;
use crate::profiles::{validate_profile_name, ProfilesState, MAX_PROFILE_NAME};
use crate::task::Task;

use super::notifications::Notifier;
use super::palette::{primary_button, Palette};

static NEXT_FORM_ID: AtomicU64 = AtomicU64::new(1);

/// What happened in a frame of `show_form`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormEvent {
    None,
    Saved,
    Cancelled,
}

pub struct ApiKeyForm {
    uid: u64,
    name: String,
    key: String,
    error: Option<String>,
    task: Option<Task<Result<(), String>>>,
    /// `Some` when editing that profile (rename and optionally replace the key).
    edit_id: Option<String>,
}

impl Default for ApiKeyForm {
    fn default() -> ApiKeyForm {
        ApiKeyForm::new()
    }
}

/// Whether the submit button is enabled.
pub fn can_submit(name: &str, key: &str, key_optional: bool, saving: bool) -> bool {
    !saving && !name.trim().is_empty() && (key_optional || !key.trim().is_empty())
}

/// Check the name against the saved profiles before asking the backend.
pub fn check_name(
    name: &str,
    profiles: &ProfilesState,
    edit_id: Option<&str>,
) -> Result<String, String> {
    validate_profile_name(name, &profiles.profiles, edit_id)
}

impl ApiKeyForm {
    /// A form that adds a profile.
    pub fn new() -> ApiKeyForm {
        ApiKeyForm {
            uid: NEXT_FORM_ID.fetch_add(1, Ordering::SeqCst),
            name: String::new(),
            key: String::new(),
            error: None,
            task: None,
            edit_id: None,
        }
    }

    /// A form that renames the profile `id` and optionally replaces its key.
    pub fn editing(id: &str, name: &str) -> ApiKeyForm {
        let mut form = ApiKeyForm::new();
        form.edit_id = Some(id.to_string());
        form.name = name.to_string();
        form
    }

    /// The add-profile form without a cancel button.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        backend: &Backend,
        ctx: &egui::Context,
        notifier: &Notifier,
        palette: &Palette,
        profiles: &ProfilesState,
        submit_label: &str,
    ) {
        let _ = self.show_form(
            ui,
            backend,
            ctx,
            notifier,
            palette,
            profiles,
            submit_label,
            false,
        );
    }

    fn poll(&mut self, notifier: &Notifier) -> FormEvent {
        let mut task = match self.task.take() {
            Some(task) => task,
            None => return FormEvent::None,
        };
        let mut result = task.poll();
        if result.is_none() && !task.is_running() {
            result = task.poll();
        }
        match result {
            Some(Ok(())) => {
                self.key.clear();
                if self.edit_id.is_none() {
                    self.name.clear();
                }
                self.error = None;
                FormEvent::Saved
            }
            Some(Err(message)) => {
                notifier.notify("Could not save the profile", &message);
                self.error = Some(message);
                FormEvent::None
            }
            None => {
                if task.is_running() {
                    self.task = Some(task);
                } else {
                    self.error = Some("Saving the profile failed unexpectedly.".to_string());
                }
                FormEvent::None
            }
        }
    }

    fn submit(&mut self, backend: &Backend, ctx: &egui::Context, profiles: &ProfilesState) {
        if self.task.is_some() {
            return;
        }
        let name = match check_name(&self.name, profiles, self.edit_id.as_deref()) {
            Ok(name) => name,
            Err(message) => {
                self.error = Some(message);
                return;
            }
        };
        self.error = None;
        let key = self.key.trim().to_string();
        let edit_id = self.edit_id.clone();
        let backend = backend.clone();
        let task: Task<Result<(), String>> = Task::spawn(ctx, move || match edit_id {
            Some(id) => {
                let credential: Option<&str> = if key.is_empty() {
                    None
                } else {
                    Some(key.as_str())
                };
                backend.update_profile(&id, Some(name.as_str()), credential)
            }
            None => backend.add_profile(&name, &key),
        });
        self.task = Some(task);
    }

    /// Draw the form; `cancelable` adds a Cancel button. Returns what happened this frame.
    #[allow(clippy::too_many_arguments)]
    pub fn show_form(
        &mut self,
        ui: &mut egui::Ui,
        backend: &Backend,
        ctx: &egui::Context,
        notifier: &Notifier,
        palette: &Palette,
        profiles: &ProfilesState,
        submit_label: &str,
        cancelable: bool,
    ) -> FormEvent {
        let mut event = self.poll(notifier);
        let key_optional = self.edit_id.is_some();
        let saving = self.task.is_some();
        let mut submit_requested = false;
        let mut cancelled = false;
        let uid = self.uid;

        ui.push_id(uid, |ui| {
            ui.label("Profile name");
            let name_response = ui.add(
                egui::TextEdit::singleline(&mut self.name)
                    .char_limit(MAX_PROFILE_NAME)
                    .hint_text("Work, Personal, ...")
                    .desired_width(f32::INFINITY),
            );
            let key_label = if key_optional {
                "Anthropic API key or Claude OAuth token (leave blank to keep the saved one)"
            } else {
                "Anthropic API key or Claude OAuth token"
            };
            ui.label(key_label);
            let key_response = ui.add(
                egui::TextEdit::singleline(&mut self.key)
                    .password(true)
                    .hint_text("sk-ant-api03-... or sk-ant-oat01-...")
                    .desired_width(f32::INFINITY),
            );
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter_pressed && (name_response.lost_focus() || key_response.lost_focus()) {
                submit_requested = true;
            }
            ui.horizontal(|ui| {
                let enabled = can_submit(&self.name, &self.key, key_optional, saving);
                let button = primary_button(palette, submit_label);
                let response = ui.add_enabled(enabled, button).on_hover_text(
                    "Saving a key for the profile in use restarts the agent, which ends any session in progress",
                );
                if response.clicked() {
                    submit_requested = true;
                }
                if cancelable && ui.button("Cancel").clicked() {
                    cancelled = true;
                }
                if saving {
                    ui.spinner();
                }
            });
            ui.label(
                RichText::new(
                    "Stored in the system keychain and sent to the sandbox when the agent starts. \
                     Run `claude setup-token` to get an OAuth token.",
                )
                .size(12.0)
                .color(palette.text_muted),
            );
            if let Some(error) = &self.error {
                ui.label(RichText::new(error.as_str()).color(palette.red));
            }
        });

        if submit_requested && can_submit(&self.name, &self.key, key_optional, saving) {
            self.submit(backend, ctx, profiles);
        }
        if cancelled {
            event = FormEvent::Cancelled;
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::ProfileInfo;

    fn profiles() -> ProfilesState {
        ProfilesState {
            profiles: vec![ProfileInfo {
                id: "a".to_string(),
                name: "Work".to_string(),
            }],
            active_id: Some("a".to_string()),
        }
    }

    #[test]
    fn submit_needs_a_name_and_a_key_unless_the_key_is_optional() {
        assert!(!can_submit("", "k", false, false));
        assert!(!can_submit("n", "  ", false, false));
        assert!(can_submit("n", "k", false, false));
        assert!(can_submit("n", "", true, false));
        assert!(!can_submit("n", "k", false, true));
    }

    #[test]
    fn names_are_checked_against_the_saved_profiles() {
        assert!(check_name("work", &profiles(), None).is_err());
        assert_eq!(
            check_name("Work", &profiles(), Some("a")),
            Ok("Work".to_string())
        );
        assert_eq!(
            check_name(" New ", &profiles(), None),
            Ok("New".to_string())
        );
    }

    #[test]
    fn editing_prefills_the_name() {
        let form = ApiKeyForm::editing("a", "Work");
        assert_eq!(form.name, "Work");
        assert!(form.key.is_empty());
        assert_ne!(form.uid, ApiKeyForm::new().uid);
    }
}
