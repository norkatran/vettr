//! Named credentials ("profiles"). Port of `src/main/profileStore.ts`, plus the in-memory active
//! profile that `src/main/index.ts` kept.
//!
//! `profiles.json` holds metadata only (id, name and the last used id). The secrets live in a
//! `SecretStore` (the OS keychain) keyed by profile id. The file is re-read from disk for every
//! operation (and operations are serialised within this store) so several running instances share
//! it without clobbering each other from stale memory.
//!
//! The legacy single-key migration is dropped: the old `apikey` file and the old per-profile
//! `credential` fields were encrypted with Electron `safeStorage`, which Rust cannot read. A
//! profile from an old `profiles.json` still lists, but has no secret until the user saves a new
//! key for it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::host::secret::{write_file_atomic, SecretStore, EMPTY_MESSAGE};
use crate::profiles::{validate_profile_name, ProfileInfo, ProfilesState};

/// Changes to a profile; `None` leaves the field as it is.
#[derive(Debug, Clone, Default)]
pub struct ProfileChanges {
    pub name: Option<String>,
    pub credential: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct StoreFile {
    last_used_id: Option<String>,
    profiles: Vec<ProfileInfo>,
}

pub struct ProfileStore {
    file: PathBuf,
    secrets: Arc<dyn SecretStore>,
    /// Serialises operations within this process.
    lock: Mutex<()>,
    /// The profile this process uses. Held in memory only, so switching it never affects other
    /// running instances; the file just remembers the last one used to seed the next launch.
    active: Mutex<Option<String>>,
}

fn parse(raw: &serde_json::Value) -> StoreFile {
    let object = match raw.as_object() {
        Some(object) => object,
        None => return StoreFile::default(),
    };
    let list = match object.get("profiles").and_then(|p| p.as_array()) {
        Some(list) => list,
        None => return StoreFile::default(),
    };
    let mut profiles: Vec<ProfileInfo> = Vec::new();
    for entry in list {
        let id = entry.get("id").and_then(|v| v.as_str());
        let name = entry.get("name").and_then(|v| v.as_str());
        if let (Some(id), Some(name)) = (id, name) {
            profiles.push(ProfileInfo {
                id: id.to_string(),
                name: name.to_string(),
            });
        }
    }
    let last_used_id = object
        .get("lastUsedId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    StoreFile {
        last_used_id,
        profiles,
    }
}

/// The trimmed name, or the user-facing error.
fn check_name(
    name: &str,
    existing: &[ProfileInfo],
    self_id: Option<&str>,
) -> Result<String, String> {
    validate_profile_name(name, existing, self_id)
}

/// The trimmed credential, or an error if it is empty.
fn trim_credential(credential: &str) -> Result<String, String> {
    let trimmed = credential.trim().to_string();
    if trimmed.is_empty() {
        return Err(EMPTY_MESSAGE.to_string());
    }
    Ok(trimmed)
}

impl ProfileStore {
    /// A store keeping `profiles.json` in `dir` (the app passes `dirs::data_dir()/vettr`).
    pub fn new(dir: &Path, secrets: Arc<dyn SecretStore>) -> Self {
        ProfileStore {
            file: dir.join("profiles.json"),
            secrets,
            lock: Mutex::new(()),
            active: Mutex::new(None),
        }
    }

    fn read(&self) -> StoreFile {
        match fs::read_to_string(&self.file) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(value) => parse(&value),
                Err(_) => StoreFile::default(),
            },
            Err(_) => StoreFile::default(),
        }
    }

    fn write(&self, data: &StoreFile) -> Result<(), String> {
        let profiles: Vec<serde_json::Value> = data
            .profiles
            .iter()
            .map(|p| serde_json::json!({ "id": p.id, "name": p.name }))
            .collect();
        let value = serde_json::json!({
            "lastUsedId": data.last_used_id,
            "profiles": profiles,
        });
        let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
        write_file_atomic(&self.file, &text, true)
    }

    pub fn list(&self) -> Vec<ProfileInfo> {
        let _guard = self.lock.lock().unwrap();
        self.read().profiles
    }

    /// The profile id new app instances start with, if it still exists.
    pub fn last_used_id(&self) -> Option<String> {
        let _guard = self.lock.lock().unwrap();
        let data = self.read();
        let last = data.last_used_id.clone()?;
        if data.profiles.iter().any(|p| p.id == last) {
            Some(last)
        } else {
            None
        }
    }

    /// Remember `id` for the next launch. Unknown ids are ignored.
    pub fn set_last_used(&self, id: &str) -> Result<(), String> {
        let _guard = self.lock.lock().unwrap();
        let mut data = self.read();
        if data.profiles.iter().any(|p| p.id == id) {
            data.last_used_id = Some(id.to_string());
            self.write(&data)?;
        }
        Ok(())
    }

    /// The saved secret, or `None` if the profile is gone or the secret cannot be read.
    pub fn credential(&self, id: &str) -> Option<String> {
        let exists = {
            let _guard = self.lock.lock().unwrap();
            self.read().profiles.iter().any(|p| p.id == id)
        };
        if !exists {
            return None;
        }
        self.secrets.get(id).unwrap_or_default()
    }

    /// Save a new profile and return its id. The error is a user-facing message.
    pub fn add(&self, name: &str, credential: &str) -> Result<String, String> {
        let _guard = self.lock.lock().unwrap();
        let mut data = self.read();
        let checked = check_name(name, &data.profiles, None)?;
        let secret = trim_credential(credential)?;
        let id = uuid::Uuid::new_v4().to_string();
        self.secrets.set(&id, &secret)?;
        data.profiles.push(ProfileInfo {
            id: id.clone(),
            name: checked,
        });
        if let Err(err) = self.write(&data) {
            let _ = self.secrets.delete(&id);
            return Err(err);
        }
        Ok(id)
    }

    /// Rename and/or replace the credential. The error is a user-facing message.
    pub fn update(&self, id: &str, changes: ProfileChanges) -> Result<(), String> {
        let _guard = self.lock.lock().unwrap();
        let mut data = self.read();
        let index = match data.profiles.iter().position(|p| p.id == id) {
            Some(index) => index,
            None => return Err("That profile no longer exists.".to_string()),
        };
        if let Some(name) = &changes.name {
            let checked = check_name(name, &data.profiles, Some(id))?;
            data.profiles[index].name = checked;
        }
        if let Some(credential) = &changes.credential {
            let secret = trim_credential(credential)?;
            self.secrets.set(id, &secret)?;
        }
        self.write(&data)
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let _guard = self.lock.lock().unwrap();
        let mut data = self.read();
        if data.last_used_id.as_deref() == Some(id) {
            data.last_used_id = None;
        }
        data.profiles.retain(|p| p.id != id);
        self.write(&data)?;
        let _ = self.secrets.delete(id);
        Ok(())
    }

    /// Seed the active profile at startup: the last used one, else the first, else none.
    pub fn init_active(&self) {
        let seed = match self.last_used_id() {
            Some(id) => Some(id),
            None => self.list().first().map(|p| p.id.clone()),
        };
        *self.active.lock().unwrap() = seed;
    }

    /// The active profile id if it still exists in the file (another instance may have removed it).
    pub fn active_id(&self) -> Option<String> {
        let current = self.active.lock().unwrap().clone();
        let id = current?;
        if self.list().iter().any(|p| p.id == id) {
            Some(id)
        } else {
            None
        }
    }

    /// The profiles and which one this process uses.
    pub fn state(&self) -> ProfilesState {
        let profiles = self.list();
        let current = self.active.lock().unwrap().clone();
        let active_id = match current {
            Some(id) if profiles.iter().any(|p| p.id == id) => Some(id),
            _ => None,
        };
        ProfilesState {
            profiles,
            active_id,
        }
    }

    /// Use `id` (or nothing) in this process and remember it for the next launch. The caller
    /// restarts the agent and notifies the UI.
    pub fn activate(&self, id: Option<&str>) -> Result<(), String> {
        *self.active.lock().unwrap() = id.map(|s| s.to_string());
        if let Some(id) = id {
            self.set_last_used(id)?;
        }
        Ok(())
    }

    /// The active profile's secret, or `None` when no profile is active or it has no usable secret.
    pub fn active_credential(&self) -> Option<String> {
        let id = self.active.lock().unwrap().clone()?;
        self.credential(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::secret::MemorySecretStore;

    fn setup() -> (tempfile::TempDir, MemorySecretStore, ProfileStore) {
        let dir = tempfile::tempdir().unwrap();
        let secrets = MemorySecretStore::new();
        let store = ProfileStore::new(dir.path(), Arc::new(secrets.clone()));
        (dir, secrets, store)
    }

    fn rename(name: &str) -> ProfileChanges {
        ProfileChanges {
            name: Some(name.to_string()),
            credential: None,
        }
    }

    fn recredential(credential: &str) -> ProfileChanges {
        ProfileChanges {
            name: None,
            credential: Some(credential.to_string()),
        }
    }

    fn info(id: &str, name: &str) -> ProfileInfo {
        ProfileInfo {
            id: id.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn starts_empty() {
        let (_dir, _secrets, store) = setup();
        assert_eq!(store.list(), Vec::<ProfileInfo>::new());
        assert_eq!(store.last_used_id(), None);
    }

    #[test]
    fn adds_profiles_keeping_the_credential_out_of_the_file() {
        let (dir, secrets, store) = setup();
        let id = store.add(" Work ", "sk-ant-secret").unwrap();
        assert_eq!(store.list(), vec![info(&id, "Work")]);
        assert_eq!(store.credential(&id), Some("sk-ant-secret".to_string()));
        assert_eq!(secrets.get(&id), Ok(Some("sk-ant-secret".to_string())));
        let text = fs::read_to_string(dir.path().join("profiles.json")).unwrap();
        assert!(!text.contains("sk-ant-secret"));
    }

    #[cfg(unix)]
    #[test]
    fn restricts_the_profiles_file_to_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, _secrets, store) = setup();
        store.add("Work", "k").unwrap();
        let mode = fs::metadata(dir.path().join("profiles.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn rejects_empty_duplicate_and_unsavable_entries() {
        let (dir, secrets, store) = setup();
        store.add("Work", "k1").unwrap();
        assert!(store
            .add("work", "k2")
            .unwrap_err()
            .contains("already exists"));
        assert!(store.add("", "k2").unwrap_err().contains("name"));
        assert!(store.add("Home", " ").unwrap_err().contains("empty"));
        assert_eq!(store.list().len(), 1);

        let other_dir = tempfile::tempdir().unwrap();
        let no_keychain = ProfileStore::new(other_dir.path(), Arc::new(secrets.unavailable()));
        assert!(no_keychain
            .add("A", "k")
            .unwrap_err()
            .contains("Secure storage"));
        assert_eq!(no_keychain.list(), Vec::<ProfileInfo>::new());
        drop(dir);
    }

    #[test]
    fn renames_and_replaces_the_credential_independently() {
        let (_dir, _secrets, store) = setup();
        let id = store.add("Work", "k1").unwrap();
        store.update(&id, rename("Job")).unwrap();
        assert_eq!(store.credential(&id), Some("k1".to_string()));
        store.update(&id, recredential("k2")).unwrap();
        assert_eq!(store.credential(&id), Some("k2".to_string()));
        assert_eq!(store.list(), vec![info(&id, "Job")]);
        let err = store.update("missing", rename("x")).unwrap_err();
        assert!(err.contains("no longer exists"));
    }

    #[test]
    fn removes_profiles_and_forgets_a_removed_last_used_id() {
        let (_dir, secrets, store) = setup();
        let a = store.add("A", "k1").unwrap();
        let b = store.add("B", "k2").unwrap();
        store.set_last_used(&b).unwrap();
        assert_eq!(store.last_used_id(), Some(b.clone()));
        store.remove(&b).unwrap();
        assert_eq!(store.last_used_id(), None);
        assert_eq!(store.credential(&b), None);
        assert_eq!(secrets.get(&b), Ok(None));
        let ids: Vec<String> = store.list().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![a]);
    }

    #[test]
    fn sees_changes_made_by_another_instance() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = MemorySecretStore::new();
        let one = ProfileStore::new(dir.path(), Arc::new(secrets.clone()));
        let two = ProfileStore::new(dir.path(), Arc::new(secrets.clone()));
        let id = one.add("A", "k1").unwrap();
        assert_eq!(two.credential(&id), Some("k1".to_string()));
        two.add("B", "k2").unwrap();
        assert_eq!(one.list().len(), 2);
    }

    #[test]
    fn returns_none_when_a_credential_cannot_be_read() {
        let (dir, secrets, store) = setup();
        let id = store.add("A", "k").unwrap();
        let broken = ProfileStore::new(dir.path(), Arc::new(secrets.failing_reads()));
        assert_eq!(broken.credential(&id), None);
    }

    #[test]
    fn ignores_malformed_files_and_entries() {
        let (dir, _secrets, store) = setup();
        let file = dir.path().join("profiles.json");
        for raw in ["not json", "null", "{}", "{\"profiles\":\"x\"}"] {
            fs::write(&file, raw).unwrap();
            assert_eq!(store.list(), Vec::<ProfileInfo>::new());
        }
        fs::write(
            &file,
            serde_json::json!({
                "lastUsedId": 5,
                "profiles": [null, { "id": "a" }, { "id": "b", "name": "B" }, { "id": 3, "name": "C" }]
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(store.list(), vec![info("b", "B")]);
        assert_eq!(store.last_used_id(), None);
    }

    #[test]
    fn lists_profiles_from_an_old_file_that_has_encrypted_credentials() {
        let (dir, _secrets, store) = setup();
        let file = dir.path().join("profiles.json");
        fs::write(
            &file,
            r#"{"lastUsedId":"b","profiles":[{"id":"b","name":"B","credential":"eA=="}]}"#,
        )
        .unwrap();
        assert_eq!(store.list(), vec![info("b", "B")]);
        assert_eq!(store.last_used_id(), Some("b".to_string()));
        assert_eq!(store.credential("b"), None);
    }

    #[test]
    fn ignores_set_last_used_for_an_unknown_profile() {
        let (_dir, _secrets, store) = setup();
        store.add("A", "k").unwrap();
        store.set_last_used("missing").unwrap();
        assert_eq!(store.last_used_id(), None);
    }

    #[test]
    fn keeps_the_last_used_id_when_removing_another_profile() {
        let (_dir, _secrets, store) = setup();
        let a = store.add("A", "k1").unwrap();
        let b = store.add("B", "k2").unwrap();
        store.set_last_used(&a).unwrap();
        store.remove(&b).unwrap();
        assert_eq!(store.last_used_id(), Some(a));
    }

    #[test]
    fn rejects_a_bad_new_name_on_update() {
        let (_dir, _secrets, store) = setup();
        let id = store.add("A", "k1").unwrap();
        store.add("B", "k2").unwrap();
        let err = store.update(&id, rename("b")).unwrap_err();
        assert!(err.contains("already exists"));
        assert_eq!(store.list()[0].name, "A");
    }

    #[test]
    fn rejects_an_empty_new_credential_on_update() {
        let (_dir, _secrets, store) = setup();
        let id = store.add("A", "k1").unwrap();
        assert!(store
            .update(&id, recredential("  "))
            .unwrap_err()
            .contains("empty"));
        assert_eq!(store.credential(&id), Some("k1".to_string()));
    }

    #[test]
    fn the_active_profile_is_seeded_from_the_last_used_one_then_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = MemorySecretStore::new();
        let store = ProfileStore::new(dir.path(), Arc::new(secrets.clone()));
        store.init_active();
        assert_eq!(store.active_id(), None);
        assert_eq!(store.active_credential(), None);

        let a = store.add("A", "k1").unwrap();
        let b = store.add("B", "k2").unwrap();
        store.init_active();
        assert_eq!(store.active_id(), Some(a.clone()));
        assert_eq!(store.active_credential(), Some("k1".to_string()));

        store.activate(Some(&b)).unwrap();
        assert_eq!(store.state().active_id, Some(b.clone()));

        // A new process starts with the profile used last, and switching is per process.
        let next = ProfileStore::new(dir.path(), Arc::new(secrets.clone()));
        next.init_active();
        assert_eq!(next.active_id(), Some(b.clone()));
        next.activate(Some(&a)).unwrap();
        assert_eq!(store.active_id(), Some(b.clone()));
    }

    #[test]
    fn the_active_profile_is_none_once_it_is_removed() {
        let (_dir, _secrets, store) = setup();
        let a = store.add("A", "k1").unwrap();
        store.activate(Some(&a)).unwrap();
        store.remove(&a).unwrap();
        assert_eq!(store.active_id(), None);
        assert_eq!(store.state().active_id, None);
        assert_eq!(store.active_credential(), None);
        store.activate(None).unwrap();
        assert_eq!(store.active_id(), None);
    }
}
