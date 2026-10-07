//! Persisted user settings. Port of `src/main/settingsStore.ts`.
//!
//! The TypeScript module kept the settings in a module-level variable; here they live in a
//! `SettingsStore` that the backend owns (wrap it in a `Mutex` to share it).

use std::fs;
use std::path::{Path, PathBuf};

use crate::host::secret::write_file_atomic;
use crate::settings::{default_settings, parse_settings, Settings};

pub struct SettingsStore {
    file: PathBuf,
    settings: Settings,
}

impl SettingsStore {
    /// A store keeping `settings.json` in `dir` (the app passes `dirs::data_dir()/vettr`). Call
    /// `load` to read what was saved.
    pub fn new(dir: &Path) -> Self {
        SettingsStore {
            file: dir.join("settings.json"),
            settings: default_settings(),
        }
    }

    /// Load the saved settings, falling back to defaults when missing or corrupt.
    pub fn load(&mut self) {
        let loaded = fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        self.settings = match loaded {
            Some(value) => parse_settings(&value),
            None => default_settings(),
        };
    }

    pub fn settings(&self) -> Settings {
        self.settings.clone()
    }

    /// Validate, store and persist; returns the settings now in effect. A failed save is logged
    /// and the settings stay in memory.
    pub fn update(&mut self, next: &serde_json::Value) -> Settings {
        self.settings = parse_settings(next);
        let result = serde_json::to_string_pretty(&self.settings)
            .map_err(|e| e.to_string())
            .and_then(|text| write_file_atomic(&self.file, &text, false));
        if let Err(err) = result {
            eprintln!("Failed to save settings: {}", err);
        }
        self.settings.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(command: &str) -> Settings {
        Settings {
            editor_command: command.to_string(),
        }
    }

    #[test]
    fn uses_defaults_when_nothing_is_saved_or_the_file_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::new(dir.path());
        store.load();
        assert_eq!(store.settings(), editor(""));
        fs::write(dir.path().join("settings.json"), "{nope").unwrap();
        store.load();
        assert_eq!(store.settings(), editor(""));
    }

    #[test]
    fn persists_updates_and_reloads_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SettingsStore::new(dir.path());
        let updated = store.update(&serde_json::json!({ "editorCommand": "zed {file}:{line}" }));
        assert_eq!(updated, editor("zed {file}:{line}"));
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(
            saved,
            serde_json::json!({ "editorCommand": "zed {file}:{line}" })
        );

        let mut reloaded = SettingsStore::new(dir.path());
        reloaded.load();
        assert_eq!(reloaded.settings().editor_command, "zed {file}:{line}");
    }

    #[test]
    fn keeps_the_settings_in_memory_when_saving_fails() {
        let root = tempfile::tempdir().unwrap();
        let blocker = root.path().join("userData");
        fs::write(&blocker, "a file, not a directory").unwrap();
        let mut store = SettingsStore::new(&blocker);
        let updated = store.update(&serde_json::json!({ "editorCommand": "x" }));
        assert_eq!(updated.editor_command, "x");
        assert_eq!(store.settings().editor_command, "x");
    }
}
