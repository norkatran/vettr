//! Where the app keeps its own data for a project on the host (port of `src/main/transcripts.ts`).

use std::path::{Path, PathBuf};

/// A stable 64-bit FNV-1a hash (std's hashers are not stable across Rust versions).
fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// The folder name of a path, with anything outside `[A-Za-z0-9_.-]` runs replaced by `_`.
fn safe_name(project: &str) -> String {
    let base = Path::new(project)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut out = String::new();
    let mut in_run = false;
    for c in base.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
            out.push(c);
            in_run = false;
        } else if !in_run {
            out.push('_');
            in_run = true;
        }
    }
    if out.is_empty() {
        "project".to_string()
    } else {
        out
    }
}

/// `<data>/projects/<name>-<hash>`. The hash of the full path keeps projects with the same
/// folder name apart; moving a project makes it a new one.
pub fn project_data_dir(data: &Path, project: &str) -> PathBuf {
    let hash = format!("{:016x}", fnv1a(project));
    data.join("projects")
        .join(format!("{}-{}", safe_name(project), &hash[..12]))
}

/// The project's Claude config dir (and so the SDK's session transcripts) on the host.
pub fn transcripts_dir(data: &Path, project: &str) -> PathBuf {
    project_data_dir(data, project).join("transcripts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_under_projects_name_hash_transcripts() {
        let dir = transcripts_dir(Path::new("/data"), "/work/my app");
        let text = dir.to_string_lossy().to_string();
        assert!(text.starts_with("/data/projects/my_app-"));
        assert!(text.ends_with("/transcripts"));
        let hash = text
            .trim_start_matches("/data/projects/my_app-")
            .trim_end_matches("/transcripts");
        assert_eq!(hash.len(), 12);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn is_stable_and_distinguishes_projects_with_the_same_folder_name() {
        let d = Path::new("/d");
        assert_eq!(transcripts_dir(d, "/a/p"), transcripts_dir(d, "/a/p"));
        assert_ne!(transcripts_dir(d, "/a/p"), transcripts_dir(d, "/b/p"));
    }

    #[test]
    fn falls_back_to_project_when_the_path_has_no_folder_name() {
        let text = transcripts_dir(Path::new("/data"), "/")
            .to_string_lossy()
            .to_string();
        assert!(text.starts_with("/data/projects/project-"));
    }
}
