//! Secrets (API keys, OAuth tokens) in the OS keychain. Port of `src/main/apiKey.ts`.
//!
//! The TypeScript version encrypted a file with Electron `safeStorage`. Here the keychain itself
//! holds the secret (service "vettr", one entry per key), so there is no secret file. There is
//! never a plain-text fallback: when the keychain is unavailable, saving fails.
//!
//! The legacy `apikey` file migration from `profileStore.ts` is dropped: that file was encrypted
//! with Electron `safeStorage`, which cannot be read from Rust, so users re-enter their key.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Service name all vettr keychain entries are stored under.
pub const KEYCHAIN_SERVICE: &str = "vettr";

/// Message when the OS keychain cannot be used.
pub const UNAVAILABLE_MESSAGE: &str = "Secure storage is not available on this system";

/// Message when asked to save an empty secret.
pub const EMPTY_MESSAGE: &str = "The API key is empty";

/// A place secrets live, keyed by a string (the profile id). Injected so tests never touch the
/// real keychain.
pub trait SecretStore: Send + Sync {
    /// The stored secret, `Ok(None)` if there is none, `Err` if it cannot be read.
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    /// Save a secret. Fails (rather than storing plain text) if secure storage is unavailable
    /// or the secret is empty.
    fn set(&self, key: &str, secret: &str) -> Result<(), String>;
    /// Remove a secret. Removing one that does not exist is fine.
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// The OS keychain through the `keyring` crate.
#[derive(Debug, Default, Clone)]
pub struct KeyringStore;

impl KeyringStore {
    pub fn new() -> Self {
        KeyringStore
    }

    fn entry(key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, key).map_err(|_| UNAVAILABLE_MESSAGE.to_string())
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let entry = KeyringStore::entry(key)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err.to_string()),
        }
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        if secret.is_empty() {
            return Err(EMPTY_MESSAGE.to_string());
        }
        let entry = KeyringStore::entry(key)?;
        entry
            .set_password(secret)
            .map_err(|err| format!("{}: {}", UNAVAILABLE_MESSAGE, err))
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        let entry = KeyringStore::entry(key)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }
}

/// An in-memory store for tests. Clones share the same data, so several stores can see each
/// other's writes like several app instances sharing one keychain.
#[derive(Debug, Clone)]
pub struct MemorySecretStore {
    data: Arc<Mutex<HashMap<String, String>>>,
    available: bool,
    fail_reads: bool,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        MemorySecretStore {
            data: Arc::new(Mutex::new(HashMap::new())),
            available: true,
            fail_reads: false,
        }
    }

    /// A view of the same data that refuses to save, like a missing keychain.
    pub fn unavailable(&self) -> Self {
        let mut other = self.clone();
        other.available = false;
        other
    }

    /// A view of the same data whose reads fail, like a keychain that changed.
    pub fn failing_reads(&self) -> Self {
        let mut other = self.clone();
        other.fail_reads = true;
        other
    }

    /// The number of stored secrets.
    pub fn len(&self) -> usize {
        self.data.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for MemorySecretStore {
    fn default() -> Self {
        MemorySecretStore::new()
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        if self.fail_reads {
            return Err("keychain changed".to_string());
        }
        Ok(self.data.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        if secret.is_empty() {
            return Err(EMPTY_MESSAGE.to_string());
        }
        if !self.available {
            return Err(UNAVAILABLE_MESSAGE.to_string());
        }
        self.data
            .lock()
            .unwrap()
            .insert(key.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.data.lock().unwrap().remove(key);
        Ok(())
    }
}

/// Write a file atomically (temp file in the same directory, then rename), creating parent
/// directories. With `private` the file is mode 0600 on unix. Shared by all the stores.
pub fn write_file_atomic(path: &Path, contents: &str, private: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(format!(".{}.tmp", std::process::id()));
    let temp = PathBuf::from(temp_name);

    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if private {
            options.mode(0o600);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = private;
    }
    let result = options
        .open(&temp)
        .and_then(|mut file| {
            file.write_all(contents.as_bytes())
                .and_then(|_| file.flush())
        })
        .and_then(|_| fs::rename(&temp, path));
    if let Err(err) = result {
        let _ = fs::remove_file(&temp);
        return Err(err.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_when_nothing_is_saved() {
        let store = MemorySecretStore::new();
        assert_eq!(store.get("a"), Ok(None));
    }

    #[test]
    fn saves_the_key_and_reads_it_back() {
        let store = MemorySecretStore::new();
        store.set("a", "sk-ant-secret").unwrap();
        assert_eq!(store.get("a"), Ok(Some("sk-ant-secret".to_string())));
        assert_eq!(store.get("b"), Ok(None));
    }

    #[test]
    fn fails_to_read_when_the_keychain_cannot_be_read() {
        let store = MemorySecretStore::new();
        store.set("a", "k").unwrap();
        assert!(store.failing_reads().get("a").is_err());
    }

    #[test]
    fn refuses_to_save_when_secure_storage_is_unavailable() {
        let store = MemorySecretStore::new().unavailable();
        let err = store.set("a", "k").unwrap_err();
        assert!(err.contains("Secure storage"));
        assert_eq!(store.get("a"), Ok(None));
    }

    #[test]
    fn refuses_to_save_an_empty_key() {
        let store = MemorySecretStore::new();
        let err = store.set("a", "").unwrap_err();
        assert!(err.contains("empty"));
        assert_eq!(store.get("a"), Ok(None));
    }

    #[test]
    fn deletes_the_key_and_deleting_twice_is_fine() {
        let store = MemorySecretStore::new();
        store.set("a", "k").unwrap();
        store.delete("a").unwrap();
        store.delete("a").unwrap();
        assert_eq!(store.get("a"), Ok(None));
    }

    #[test]
    fn writes_atomically_with_private_mode_and_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nested").join("secret.json");
        write_file_atomic(&file, "one", true).unwrap();
        write_file_atomic(&file, "two", true).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "two");
        let leftovers: Vec<_> = fs::read_dir(file.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
