//! User settings persisted on the host (port of `src/shared/settings.ts`). Add new fields here
//! with a default.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[derive(Default)]
pub struct Settings {
    /// Command template for "Open in editor"; `{file}`, `{line}` and `{project}` (the project
    /// folder) are substituted. Empty means unset.
    pub editor_command: String,
}

/// `defaultSettings` in the TS.
pub fn default_settings() -> Settings {
    Settings::default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EditorPreset {
    pub label: &'static str,
    pub command: &'static str,
}

pub static EDITOR_PRESETS: [EditorPreset; 5] = [
    EditorPreset {
        label: "VS Code",
        command: "code {project} -g {file}:{line}",
    },
    EditorPreset {
        label: "Cursor",
        command: "cursor {project} -g {file}:{line}",
    },
    EditorPreset {
        label: "Zed",
        command: "zed {project} {file}:{line}",
    },
    EditorPreset {
        label: "Sublime Text",
        command: "subl {project} {file}:{line}",
    },
    EditorPreset {
        label: "IntelliJ IDEA",
        command: "idea {project} --line {line} {file}",
    },
];

/// The preset whose command equals `command`, or `None` when it is empty or custom.
pub fn match_preset(command: &str) -> Option<&'static EditorPreset> {
    EDITOR_PRESETS.iter().find(|p| p.command == command)
}

/// Validate untrusted JSON read from disk, falling back to defaults per field.
pub fn parse_settings(raw: &serde_json::Value) -> Settings {
    match raw.as_object() {
        None => default_settings(),
        Some(obj) => {
            let editor_command = match obj.get("editorCommand") {
                Some(serde_json::Value::String(s)) => s.trim().to_string(),
                _ => String::new(),
            };
            Settings { editor_command }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_falls_back_to_defaults_for_non_objects() {
        assert_eq!(parse_settings(&json!(null)), default_settings());
        assert_eq!(parse_settings(&json!("x")), default_settings());
    }

    #[test]
    fn parse_keeps_a_trimmed_editor_command() {
        assert_eq!(
            parse_settings(&json!({ "editorCommand": "  code -g {file}:{line} " })),
            Settings {
                editor_command: "code -g {file}:{line}".to_string()
            }
        );
    }

    #[test]
    fn parse_ignores_a_wrongly_typed_editor_command() {
        assert_eq!(
            parse_settings(&json!({ "editorCommand": 3 })),
            default_settings()
        );
    }

    #[test]
    fn match_preset_finds_a_preset_by_its_command() {
        let first = &EDITOR_PRESETS[0];
        assert_eq!(match_preset(first.command), Some(first));
    }

    #[test]
    fn match_preset_returns_none_for_empty_or_custom_commands() {
        assert_eq!(match_preset(""), None);
        assert_eq!(match_preset("nano {file}"), None);
    }
}
