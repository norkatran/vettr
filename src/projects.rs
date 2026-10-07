//! Persisted project state: the current project plus recently opened ones (port of `src/shared/projects.ts`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectState {
    pub current: Option<String>,
    /// Most recent first, always includes `current` when set.
    pub recent: Vec<String>,
}

pub const MAX_RECENT_PROJECTS: usize = 10;

/// `emptyProjectState` in the TS.
pub fn empty_project_state() -> ProjectState {
    ProjectState {
        current: None,
        recent: Vec::new(),
    }
}

impl Default for ProjectState {
    fn default() -> ProjectState {
        empty_project_state()
    }
}

/// Make `path` the current project and move it to the front of the recent list.
pub fn open_project_state(state: &ProjectState, path: &str) -> ProjectState {
    let mut recent: Vec<String> = vec![path.to_string()];
    for p in &state.recent {
        if p != path {
            recent.push(p.clone());
        }
    }
    recent.truncate(MAX_RECENT_PROJECTS);
    ProjectState {
        current: Some(path.to_string()),
        recent,
    }
}

/// Forget a path (for example a folder that no longer exists).
pub fn remove_project_state(state: &ProjectState, path: &str) -> ProjectState {
    let current = if state.current.as_deref() == Some(path) {
        None
    } else {
        state.current.clone()
    };
    ProjectState {
        current,
        recent: state
            .recent
            .iter()
            .filter(|p| p.as_str() != path)
            .cloned()
            .collect(),
    }
}

/// Validate untrusted JSON read from disk, falling back to empty state.
pub fn parse_project_state(raw: &serde_json::Value) -> ProjectState {
    let obj = match raw.as_object() {
        Some(o) => o,
        None => return empty_project_state(),
    };
    let mut unique: Vec<String> = Vec::new();
    if let Some(serde_json::Value::Array(items)) = obj.get("recent") {
        for item in items {
            if let serde_json::Value::String(s) = item {
                if !s.is_empty() && !unique.contains(s) {
                    unique.push(s.clone());
                }
            }
        }
    }
    unique.truncate(MAX_RECENT_PROJECTS);
    let current = match obj.get("current") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    ProjectState {
        current,
        recent: unique,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn open_sets_the_current_project_and_puts_it_first() {
        let a = open_project_state(&empty_project_state(), "/a");
        let b = open_project_state(&a, "/b");
        assert_eq!(
            b,
            ProjectState {
                current: Some("/b".to_string()),
                recent: strs(&["/b", "/a"]),
            }
        );
    }

    #[test]
    fn open_moves_a_reopened_project_to_the_front_without_duplicating_it() {
        let mut s = open_project_state(&empty_project_state(), "/a");
        s = open_project_state(&s, "/b");
        s = open_project_state(&s, "/a");
        assert_eq!(s.recent, strs(&["/a", "/b"]));
    }

    #[test]
    fn open_caps_the_recent_list() {
        let mut s = empty_project_state();
        for i in 0..(MAX_RECENT_PROJECTS + 5) {
            s = open_project_state(&s, &format!("/p{}", i));
        }
        assert_eq!(s.recent.len(), MAX_RECENT_PROJECTS);
        assert_eq!(s.recent[0], format!("/p{}", MAX_RECENT_PROJECTS + 4));
    }

    #[test]
    fn remove_clears_current_when_it_is_the_removed_path() {
        let s = open_project_state(&empty_project_state(), "/a");
        assert_eq!(remove_project_state(&s, "/a"), empty_project_state());
    }

    #[test]
    fn remove_keeps_current_when_removing_another_path() {
        let mut s = open_project_state(&empty_project_state(), "/a");
        s = open_project_state(&s, "/b");
        assert_eq!(
            remove_project_state(&s, "/a"),
            ProjectState {
                current: Some("/b".to_string()),
                recent: strs(&["/b"]),
            }
        );
    }

    #[test]
    fn parse_falls_back_to_empty_state_for_bad_input() {
        assert_eq!(parse_project_state(&json!(null)), empty_project_state());
        assert_eq!(parse_project_state(&json!("x")), empty_project_state());
        assert_eq!(
            parse_project_state(&json!({ "current": 5, "recent": "nope" })),
            empty_project_state()
        );
    }

    #[test]
    fn parse_drops_invalid_and_duplicate_entries() {
        assert_eq!(
            parse_project_state(&json!({ "current": "/a", "recent": ["/a", 1, "", "/a", "/b"] })),
            ProjectState {
                current: Some("/a".to_string()),
                recent: strs(&["/a", "/b"]),
            }
        );
    }
}
