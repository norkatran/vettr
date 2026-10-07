//! Persisted project state (current project and recents). Port of `src/main/projectStore.ts`.
//!
//! The TypeScript module kept the state in a module-level variable; here the state lives in a
//! `ProjectStore` that the backend owns (wrap it in a `Mutex` to share it).

use std::fs;
use std::path::{Path, PathBuf};

use crate::host::secret::write_file_atomic;
use crate::projects::{
    empty_project_state, open_project_state, parse_project_state, remove_project_state,
    ProjectState,
};

pub struct ProjectStore {
    file: PathBuf,
    state: ProjectState,
}

impl ProjectStore {
    /// A store keeping `projects.json` in `dir` (the app passes `dirs::data_dir()/vettr`). Call
    /// `load` to read what was saved.
    pub fn new(dir: &Path) -> Self {
        ProjectStore {
            file: dir.join("projects.json"),
            state: empty_project_state(),
        }
    }

    fn save(&self) {
        let result = serde_json::to_string_pretty(&self.state)
            .map_err(|e| e.to_string())
            .and_then(|text| write_file_atomic(&self.file, &text, false));
        if let Err(err) = result {
            eprintln!("Failed to save project state: {}", err);
        }
    }

    /// Load persisted state, dropping folders that no longer exist.
    pub fn load(&mut self) {
        let loaded = fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        self.state = match loaded {
            Some(value) => parse_project_state(&value),
            None => empty_project_state(),
        };
        let recent: Vec<String> = self.state.recent.clone();
        for path in recent {
            if !Path::new(&path).exists() {
                self.state = remove_project_state(&self.state, &path);
            }
        }
        if let Some(current) = self.state.current.clone() {
            if !Path::new(&current).exists() {
                self.state.current = None;
            }
        }
    }

    pub fn state(&self) -> ProjectState {
        self.state.clone()
    }

    pub fn set_current_project(&mut self, path: &str) {
        self.state = open_project_state(&self.state, path);
        self.save();
    }

    pub fn forget_project(&mut self, path: &str) {
        self.state = remove_project_state(&self.state, path);
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Env {
        _root: tempfile::TempDir,
        root: PathBuf,
        user_data: PathBuf,
    }

    fn env() -> Env {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().to_path_buf();
        let user_data = root_path.join("userData");
        fs::create_dir(&user_data).unwrap();
        Env {
            _root: root,
            root: root_path,
            user_data,
        }
    }

    fn make_dir(env: &Env, name: &str) -> String {
        let dir = env.root.join(name);
        fs::create_dir(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    fn state_file(env: &Env) -> PathBuf {
        env.user_data.join("projects.json")
    }

    fn state_of(current: Option<&str>, recent: &[&str]) -> ProjectState {
        ProjectState {
            current: current.map(|s| s.to_string()),
            recent: recent.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn starts_empty_when_nothing_has_been_saved() {
        let env = env();
        let mut store = ProjectStore::new(&env.user_data);
        store.load();
        assert_eq!(store.state(), state_of(None, &[]));
    }

    #[test]
    fn persists_the_current_project_and_recents_to_disk() {
        let env = env();
        let mut store = ProjectStore::new(&env.user_data);
        store.load();
        let a = make_dir(&env, "a");
        store.set_current_project(&a);
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(state_file(&env)).unwrap()).unwrap();
        assert_eq!(saved, serde_json::json!({ "current": a, "recent": [a] }));
    }

    #[test]
    fn restores_the_last_opened_project_after_a_restart() {
        let env = env();
        let mut store = ProjectStore::new(&env.user_data);
        store.load();
        let a = make_dir(&env, "a");
        let b = make_dir(&env, "b");
        store.set_current_project(&a);
        store.set_current_project(&b);

        let mut restarted = ProjectStore::new(&env.user_data);
        restarted.load();
        assert_eq!(restarted.state(), state_of(Some(&b), &[&b, &a]));
    }

    #[test]
    fn changing_project_makes_the_new_one_the_default() {
        let env = env();
        let mut store = ProjectStore::new(&env.user_data);
        store.load();
        let a = make_dir(&env, "a");
        let b = make_dir(&env, "b");
        store.set_current_project(&a);
        store.set_current_project(&b);
        store.set_current_project(&a);
        store.load();
        assert_eq!(store.state().current, Some(a.clone()));
        assert_eq!(store.state().recent, vec![a, b]);
    }

    #[test]
    fn drops_folders_that_no_longer_exist_on_load() {
        let env = env();
        let mut store = ProjectStore::new(&env.user_data);
        store.load();
        let a = make_dir(&env, "a");
        let b = make_dir(&env, "b");
        store.set_current_project(&a);
        store.set_current_project(&b);
        fs::remove_dir_all(&b).unwrap();

        store.load();
        assert_eq!(store.state(), state_of(None, &[&a]));
    }

    #[test]
    fn forget_project_removes_it_and_persists_the_change() {
        let env = env();
        let mut store = ProjectStore::new(&env.user_data);
        store.load();
        let a = make_dir(&env, "a");
        let b = make_dir(&env, "b");
        store.set_current_project(&a);
        store.set_current_project(&b);
        store.forget_project(&a);
        store.load();
        assert_eq!(store.state(), state_of(Some(&b), &[&b]));
    }

    #[test]
    fn recovers_from_a_corrupt_state_file() {
        let env = env();
        fs::write(state_file(&env), "{not json").unwrap();
        let mut store = ProjectStore::new(&env.user_data);
        store.load();
        assert_eq!(store.state(), state_of(None, &[]));
    }

    #[test]
    fn keeps_in_memory_state_when_saving_fails() {
        let env = env();
        let a = make_dir(&env, "a");
        // A file where the data directory should be makes creating the directory and writing fail.
        let blocker = env.root.join("blocker");
        fs::write(&blocker, "").unwrap();
        let mut store = ProjectStore::new(&blocker);
        store.load();
        store.set_current_project(&a);
        assert_eq!(store.state(), state_of(Some(&a), &[&a]));
    }

    #[test]
    fn clears_a_missing_current_project_even_if_it_is_not_in_the_recent_list() {
        let env = env();
        let a = make_dir(&env, "a");
        let gone = env.root.join("gone").to_string_lossy().to_string();
        fs::write(
            state_file(&env),
            serde_json::json!({ "current": gone, "recent": [a] }).to_string(),
        )
        .unwrap();
        let mut store = ProjectStore::new(&env.user_data);
        store.load();
        assert_eq!(store.state(), state_of(None, &[&a]));
    }
}
