//! Storage for secrets that must not live in the database.
//!
//! Two backends:
//!
//! * [`KeyringSecretStore`] - the operating system keychain (GNOME Keyring / KWallet via
//!   Secret Service on Linux, Credential Manager on Windows, Keychain on macOS). This is
//!   the intended backend and the default.
//! * [`LocalFileSecretStore`] - a fallback for machines with no keychain daemon, such as
//!   a bare Kali install with no session keyring. It encrypts secrets with a key kept in
//!   a sibling file with owner-only permissions.
//!
//! The fallback is deliberately honest about what it is: it stops a secret appearing in
//! plain text in a backup, a screen share or a `grep` of the data directory. It does not
//! protect against an attacker who can already read files as this user. The UI shows
//! which backend is active so nobody has to guess.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::security::crypto::{self, VaultKey};

/// Keychain service name. Entries appear under this in the OS credential UI.
pub const SERVICE: &str = "AuditForge";

/// Key for the OpenAI API key.
pub const KEY_OPENAI_API: &str = "openai_api_key";
/// Key for the evidence vault encryption key.
pub const KEY_EVIDENCE_VAULT: &str = "evidence_vault_key";

pub trait SecretStore: Send + Sync {
    fn set(&self, key: &str, value: &str) -> Result<()>;
    /// `Ok(None)` means "no such secret", which is not an error.
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn delete(&self, key: &str) -> Result<()>;
    /// Name shown in Settings, e.g. "OS keychain".
    fn backend_name(&self) -> &'static str;
    /// False for the fallback, so the UI can warn.
    fn is_os_backed(&self) -> bool;
}

impl dyn SecretStore {
    pub fn has(&self, key: &str) -> bool {
        matches!(self.get(key), Ok(Some(_)))
    }
}

/// Pick the best available backend, falling back if the keychain cannot be reached.
pub fn open_default(data_dir: &Path) -> Box<dyn SecretStore> {
    #[cfg(feature = "os-keyring")]
    {
        let store = KeyringSecretStore::new();
        // A keychain that is present but locked, or absent entirely, shows up as an
        // error on the first round trip rather than at construction time.
        if store.self_test().is_ok() {
            return Box::new(store);
        }
        tracing::warn!("OS keychain unavailable; falling back to the encrypted local secret file");
    }
    Box::new(LocalFileSecretStore::new(data_dir.join("secrets")))
}

#[cfg(feature = "os-keyring")]
pub struct KeyringSecretStore;

#[cfg(feature = "os-keyring")]
impl Default for KeyringSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "os-keyring")]
impl KeyringSecretStore {
    pub fn new() -> Self {
        KeyringSecretStore
    }

    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, key)
            .map_err(|e| Error::internal(format!("keychain unavailable: {e}")))
    }

    /// Write and delete a throwaway entry to prove the keychain actually works.
    pub fn self_test(&self) -> Result<()> {
        let probe = "__auditforge_probe";
        self.set(probe, "ok")?;
        let value = self.get(probe)?;
        let _ = self.delete(probe);
        if value.as_deref() == Some("ok") {
            Ok(())
        } else {
            Err(Error::internal("keychain round trip failed"))
        }
    }
}

#[cfg(feature = "os-keyring")]
impl SecretStore for KeyringSecretStore {
    fn set(&self, key: &str, value: &str) -> Result<()> {
        Self::entry(key)?
            .set_password(value)
            .map_err(|e| Error::internal(format!("could not store secret: {e}")))
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::internal(format!("could not read secret: {e}"))),
        }
    }

    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::internal(format!("could not remove secret: {e}"))),
        }
    }

    fn backend_name(&self) -> &'static str {
        "OS keychain"
    }

    fn is_os_backed(&self) -> bool {
        true
    }
}

/// Encrypted file fallback. See the module docs for exactly how much this protects.
pub struct LocalFileSecretStore {
    dir: PathBuf,
}

impl LocalFileSecretStore {
    pub fn new(dir: PathBuf) -> Self {
        LocalFileSecretStore { dir }
    }

    fn device_key(&self) -> Result<VaultKey> {
        let key_path = self.dir.join("device.key");
        if key_path.exists() {
            let encoded = std::fs::read_to_string(&key_path)?;
            return VaultKey::from_base64(&encoded);
        }
        std::fs::create_dir_all(&self.dir)?;
        let key = VaultKey::generate();
        write_private(&key_path, key.to_base64().as_bytes())?;
        Ok(key)
    }

    fn path_for(&self, key: &str) -> PathBuf {
        // Secret names are internal constants, never user input, but sanitize anyway so
        // a future caller cannot turn one into a path.
        self.dir
            .join(format!("{}.bin", super::paths::sanitize_filename(key)))
    }
}

impl SecretStore for LocalFileSecretStore {
    fn set(&self, key: &str, value: &str) -> Result<()> {
        let device_key = self.device_key()?;
        let blob = crypto::encrypt(&device_key, value.as_bytes(), key.as_bytes())?;
        std::fs::create_dir_all(&self.dir)?;
        write_private(&self.path_for(key), &blob)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        let path = self.path_for(key);
        if !path.exists() {
            return Ok(None);
        }
        let blob = std::fs::read(&path)?;
        let device_key = self.device_key()?;
        let plaintext = crypto::decrypt(&device_key, &blob, key.as_bytes())?;
        Ok(Some(String::from_utf8_lossy(&plaintext).into_owned()))
    }

    fn delete(&self, key: &str) -> Result<()> {
        let path = self.path_for(key);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "Encrypted local file"
    }

    fn is_os_backed(&self) -> bool {
        false
    }
}

/// In-memory store used by tests and by `--no-secrets` runs. Never persists.
#[derive(Default)]
pub struct MemorySecretStore {
    inner: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.inner.lock().unwrap().get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.inner.lock().unwrap().remove(key);
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "In-memory (not persisted)"
    }

    fn is_os_backed(&self) -> bool {
        false
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_store_round_trips_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFileSecretStore::new(dir.path().join("secrets"));
        assert_eq!(store.get(KEY_OPENAI_API).unwrap(), None);

        store.set(KEY_OPENAI_API, "sk-test-value").unwrap();
        assert_eq!(
            store.get(KEY_OPENAI_API).unwrap().as_deref(),
            Some("sk-test-value")
        );

        store.delete(KEY_OPENAI_API).unwrap();
        assert_eq!(store.get(KEY_OPENAI_API).unwrap(), None);
        // Deleting something that is not there is not an error.
        store.delete(KEY_OPENAI_API).unwrap();
    }

    #[test]
    fn local_store_never_writes_the_secret_in_clear() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFileSecretStore::new(dir.path().join("secrets"));
        store.set(KEY_OPENAI_API, "sk-plaintext-canary").unwrap();

        let mut found = false;
        for entry in std::fs::read_dir(dir.path().join("secrets")).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            assert!(
                !String::from_utf8_lossy(&bytes).contains("sk-plaintext-canary"),
                "secret found in clear on disk"
            );
            found = true;
        }
        assert!(found, "expected the store to have written something");
    }

    #[test]
    fn a_secret_file_cannot_be_read_under_a_different_name() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = dir.path().join("secrets");
        let store = LocalFileSecretStore::new(secrets.clone());
        store.set(KEY_OPENAI_API, "sk-test-value").unwrap();

        // The secret name is authenticated, so renaming the file breaks decryption
        // instead of silently returning the wrong secret.
        std::fs::rename(
            secrets.join(format!("{KEY_OPENAI_API}.bin")),
            secrets.join(format!("{KEY_EVIDENCE_VAULT}.bin")),
        )
        .unwrap();
        assert!(store.get(KEY_EVIDENCE_VAULT).is_err());
    }

    #[test]
    fn memory_store_behaves_like_the_others() {
        let store = MemorySecretStore::default();
        assert_eq!(store.get("k").unwrap(), None);
        store.set("k", "v").unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some("v"));
        store.delete("k").unwrap();
        assert_eq!(store.get("k").unwrap(), None);
        assert!(!store.is_os_backed());
    }

    #[test]
    fn open_default_always_returns_a_usable_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_default(dir.path());
        store.set("probe", "value").unwrap();
        assert_eq!(store.get("probe").unwrap().as_deref(), Some("value"));
        store.delete("probe").unwrap();
        assert!(!store.backend_name().is_empty());
    }
}
