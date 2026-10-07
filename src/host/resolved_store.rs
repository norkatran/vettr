//! The ids of the comments the user resolved in a project, stored in its data dir. Port of
//! `src/main/resolvedStore.ts`.

use std::fs;
use std::path::Path;

use crate::host::secret::write_file_atomic;
use crate::resolution::{parse_resolved, with_resolved};

const FILE: &str = "resolved-comments.json";

/// The ids of the comments the user resolved in a project.
pub fn read_resolved(data_dir: &Path) -> Vec<String> {
    let loaded = fs::read_to_string(data_dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    match loaded {
        Some(value) => parse_resolved(&value),
        None => Vec::new(),
    }
}

/// Mark a comment resolved or reopened; returns the ids now resolved (unchanged on failure).
pub fn set_resolved(data_dir: &Path, id: &str, resolved: bool) -> Vec<String> {
    let before = read_resolved(data_dir);
    let after: Vec<String> = with_resolved(&before, id, resolved);
    if after == before {
        return before;
    }
    let result = serde_json::to_string_pretty(&after)
        .map_err(|e| e.to_string())
        .and_then(|text| write_file_atomic(&data_dir.join(FILE), &text, false));
    match result {
        Ok(()) => after,
        Err(err) => {
            eprintln!("Failed to save resolved comments: {}", err);
            before
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_resolved(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn persists_resolving_and_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert_eq!(set_resolved(d, "a", true), ids(&["a"]));
        assert_eq!(set_resolved(d, "b", true), ids(&["a", "b"]));
        assert_eq!(read_resolved(d), ids(&["a", "b"]));
        assert_eq!(set_resolved(d, "a", false), ids(&["b"]));
        assert_eq!(read_resolved(d), ids(&["b"]));
    }

    #[test]
    fn does_not_touch_the_file_when_nothing_changes() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert_eq!(set_resolved(d, "a", false), Vec::<String>::new());
        assert!(!d.join("resolved-comments.json").exists());
        set_resolved(d, "a", true);
        assert_eq!(set_resolved(d, "a", true), ids(&["a"]));
    }

    #[test]
    fn creates_the_data_dir_when_needed() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("nested").join("project");
        assert_eq!(set_resolved(&d, "a", true), ids(&["a"]));
        assert_eq!(read_resolved(&d), ids(&["a"]));
    }

    #[test]
    fn treats_a_corrupt_file_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("resolved-comments.json"), "{nope").unwrap();
        assert_eq!(read_resolved(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn returns_the_old_list_and_does_not_panic_when_it_cannot_save() {
        // A regular file where the data dir should be, so creating or writing under it fails
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-dir");
        fs::write(&blocker, "").unwrap();
        assert_eq!(set_resolved(&blocker, "a", true), Vec::<String>::new());
    }
}
