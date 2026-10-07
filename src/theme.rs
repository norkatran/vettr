//! Theme choice logic (port of `src/shared/theme.ts`). The egui `Visuals` live in the UI.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    /// The persisted string (`light` or `dark`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
}

/// The theme to show: an explicit choice wins, otherwise follow the OS setting.
pub fn resolve_theme(choice: Option<Theme>, system_dark: bool) -> Theme {
    match choice {
        Some(t) => t,
        None => {
            if system_dark {
                Theme::Dark
            } else {
                Theme::Light
            }
        }
    }
}

/// Narrow a stored value to a theme choice; anything unrecognised means "follow the OS".
pub fn parse_theme_choice(value: Option<&str>) -> Option<Theme> {
    match value {
        Some("light") => Some(Theme::Light),
        Some("dark") => Some(Theme::Dark),
        _ => None,
    }
}

pub fn other_theme(theme: Theme) -> Theme {
    match theme {
        Theme::Dark => Theme::Light,
        Theme::Light => Theme::Dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_follows_the_os_when_there_is_no_explicit_choice() {
        assert_eq!(resolve_theme(None, true), Theme::Dark);
        assert_eq!(resolve_theme(None, false), Theme::Light);
    }

    #[test]
    fn resolve_prefers_an_explicit_choice_over_the_os() {
        assert_eq!(resolve_theme(Some(Theme::Light), true), Theme::Light);
        assert_eq!(resolve_theme(Some(Theme::Dark), false), Theme::Dark);
    }

    #[test]
    fn parse_accepts_the_two_themes_and_rejects_anything_else() {
        assert_eq!(parse_theme_choice(Some("light")), Some(Theme::Light));
        assert_eq!(parse_theme_choice(Some("dark")), Some(Theme::Dark));
        assert_eq!(parse_theme_choice(Some("solarized")), None);
        assert_eq!(parse_theme_choice(None), None);
    }

    #[test]
    fn other_theme_flips_between_light_and_dark() {
        assert_eq!(other_theme(Theme::Dark), Theme::Light);
        assert_eq!(other_theme(Theme::Light), Theme::Dark);
    }
}
