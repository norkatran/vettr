//! Saved credential profiles as the UI sees them (port of `src/shared/profiles.ts`): the secret
//! itself never leaves the host.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesState {
    pub profiles: Vec<ProfileInfo>,
    /// The profile this app instance uses, or `None` when none is selected.
    pub active_id: Option<String>,
}

pub const MAX_PROFILE_NAME: usize = 40;

/// The trimmed name, or an error message when it is empty, too long or already used by another
/// profile (other than `self_id`, the profile being renamed).
pub fn validate_profile_name(
    name: &str,
    existing: &[ProfileInfo],
    self_id: Option<&str>,
) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Give the profile a name.".to_string());
    }
    // JS counts UTF-16 code units
    if trimmed.encode_utf16().count() > MAX_PROFILE_NAME {
        return Err(format!(
            "Use at most {} characters for the name.",
            MAX_PROFILE_NAME
        ));
    }
    let lower = trimmed.to_lowercase();
    let taken = existing
        .iter()
        .any(|p| Some(p.id.as_str()) != self_id && p.name.to_lowercase() == lower);
    if taken {
        return Err(format!("A profile named \"{}\" already exists.", trimmed));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing() -> Vec<ProfileInfo> {
        vec![ProfileInfo {
            id: "a".to_string(),
            name: "Work".to_string(),
        }]
    }

    #[test]
    fn trims() {
        assert_eq!(
            validate_profile_name("  Home ", &existing(), None),
            Ok("Home".to_string())
        );
    }

    #[test]
    fn rejects_empty_names() {
        assert!(validate_profile_name("  ", &existing(), None).is_err());
    }

    #[test]
    fn rejects_long_names() {
        assert!(validate_profile_name(&"x".repeat(41), &existing(), None).is_err());
    }

    #[test]
    fn rejects_duplicates_ignoring_case() {
        assert!(validate_profile_name("work", &existing(), None).is_err());
    }

    #[test]
    fn lets_a_profile_keep_its_own_name() {
        assert_eq!(
            validate_profile_name("Work", &existing(), Some("a")),
            Ok("Work".to_string())
        );
    }
}
