//! Where the cookie string itself lives.
//!
//! The SQLite store holds account metadata only. The cookie — the
//! actual credential — lives in the OS keyring under the account id
//! (Windows Credential Manager, macOS Keychain, Linux Secret
//! Service), through the [`KeyringStore`].
//!
//! When no keyring is available at runtime (no secret service on the
//! machine, a locked store, a headless session), [`open_secret_store`]
//! falls back to [`EncryptedFileStore`]: AES-256-GCM files under a
//! directory the app owns, with the key in a separate file next to
//! them. The fallback's limits, stated honestly:
//!
//! - The key file sits next to the ciphertext, both mode 0600. That
//!   stops other local users and casual snooping, but NOT any process
//!   running as the same user (or root): it can read both files and
//!   recover the cookies. It is not a substitute for a real keyring.
//! - The key is never backed up or synced separately; losing the
//!   directory loses the cookies (re-sign-in fixes it).
//! - The fallback is used only when the keyring probe fails, and the
//!   caller is told (see [`SecretBackend`]) so the UI can say so.
//!
//! The trait is object-safe and blocking (keyring and file I/O are
//! both blocking calls); callers run it off the async runtime's
//! blocking pool or, for the short calls here, inline.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::{Error, SecretString};

/// The service name the keyring entries are stored under — the app's
/// bundle identifier, so the entries are recognizable in the OS
/// credential manager.
const KEYRING_SERVICE: &str = "com.tubelite.desktop";

/// Which backend the secret store is using.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretBackend {
    /// The OS keyring (Credential Manager / Keychain / Secret Service).
    Keyring,
    /// AES-256-GCM files under an app-owned directory (see the module
    /// docs for the limits).
    EncryptedFile,
}

/// A store for one secret per account id. All methods are blocking.
pub trait SecretStore: Send + Sync {
    /// The stored secret for `id`, or `None` when there is none.
    fn get(&self, id: &str) -> Result<Option<SecretString>, Error>;
    /// Store (or replace) the secret for `id`.
    fn set(&self, id: &str, secret: &SecretString) -> Result<(), Error>;
    /// Delete the secret for `id`. Deleting a missing entry is fine.
    fn delete(&self, id: &str) -> Result<(), Error>;
    /// Human-readable backend name, for status display.
    fn backend_name(&self) -> &'static str;
}

/// Probe the OS keyring and return a store for it, or fall back to
/// the encrypted-file store under `dir` when no keyring is available.
/// Returns the store and which backend was chosen; the caller warns
/// the user when the fallback was chosen.
///
/// The probe (`keyring::Entry::store_status`) only initializes the
/// default store — it writes nothing.
pub fn open_secret_store(dir: &Path) -> (std::sync::Arc<dyn SecretStore>, SecretBackend) {
    match keyring::Entry::store_status() {
        Ok(()) => (std::sync::Arc::new(KeyringStore), SecretBackend::Keyring),
        Err(_) => {
            tracing::warn!(
                "no OS keyring available; cookies will be stored in an encrypted file under {} \
                 (see crates/accounts/src/secrets.rs for the limits)",
                dir.display()
            );
            (
                std::sync::Arc::new(EncryptedFileStore::new(dir)),
                SecretBackend::EncryptedFile,
            )
        }
    }
}

/// The OS keyring, one entry per account id.
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn get(&self, id: &str) -> Result<Option<SecretString>, Error> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, id).map_err(Error::from_keyring)?;
        match entry.get_password() {
            Ok(password) => Ok(Some(SecretString::new(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::from_keyring(e)),
        }
    }

    fn set(&self, id: &str, secret: &SecretString) -> Result<(), Error> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, id).map_err(Error::from_keyring)?;
        entry
            .set_password(secret.expose())
            .map_err(Error::from_keyring)
    }

    fn delete(&self, id: &str) -> Result<(), Error> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, id).map_err(Error::from_keyring)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::from_keyring(e)),
        }
    }

    fn backend_name(&self) -> &'static str {
        "OS keyring"
    }
}

/// AES-256-GCM files under `dir`, with the key in `dir/key.bin`.
/// See the module docs for what this does and does not protect
/// against.
pub struct EncryptedFileStore {
    dir: PathBuf,
}

impl EncryptedFileStore {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn key_path(&self) -> PathBuf {
        self.dir.join("key.bin")
    }

    fn entry_path(&self, id: &str) -> PathBuf {
        // The id is `ga-<hex>`, so it is safe as a file name.
        self.dir.join(format!("{id}.enc"))
    }

    /// Load the 32-byte key, creating it (mode 0600) on first use.
    fn key(&self) -> Result<[u8; 32], Error> {
        let path = self.key_path();
        if let Ok(bytes) = std::fs::read(&path) {
            if bytes.len() != 32 {
                return Err(Error::Crypto("secret key file is corrupt"));
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
        std::fs::create_dir_all(&self.dir)?;
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|_| Error::Crypto("no randomness available"))?;
        write_mode_0600(&path, &key)?;
        Ok(key)
    }

    fn cipher(key: &[u8; 32]) -> aes_gcm::Aes256Gcm {
        use aes_gcm::{aead::KeyInit, Key};
        aes_gcm::Aes256Gcm::new(Key::<aes_gcm::Aes256Gcm>::from_slice(key))
    }
}

impl SecretStore for EncryptedFileStore {
    fn get(&self, id: &str) -> Result<Option<SecretString>, Error> {
        let path = self.entry_path(id);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::Io(e)),
        };
        if bytes.len() < 12 {
            return Err(Error::Crypto("stored secret file is corrupt"));
        }
        let (nonce, ciphertext) = bytes.split_at(12);
        let key = self.key()?;
        let cipher = Self::cipher(&key);
        use aes_gcm::{aead::Aead, Nonce};
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| Error::Crypto("stored secret is unreadable"))?;
        let text = String::from_utf8(plaintext)
            .map_err(|_| Error::Crypto("stored secret is not valid UTF-8"))?;
        Ok(Some(SecretString::new(text)))
    }

    fn set(&self, id: &str, secret: &SecretString) -> Result<(), Error> {
        let key = self.key()?;
        let cipher = Self::cipher(&key);
        let mut nonce_bytes = [0u8; 12];
        getrandom::fill(&mut nonce_bytes).map_err(|_| Error::Crypto("no randomness available"))?;
        use aes_gcm::{aead::Aead, Nonce};
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), secret.expose().as_bytes())
            .map_err(|_| Error::Crypto("could not encrypt the secret"))?;
        let mut bytes = nonce_bytes.to_vec();
        bytes.extend_from_slice(&ciphertext);
        std::fs::create_dir_all(&self.dir)?;
        // Write to a temp file and rename, so a crash cannot leave a
        // half-written secret behind.
        let tmp = self.dir.join(format!(".{id}.enc.tmp"));
        write_mode_0600(&tmp, &bytes)?;
        std::fs::rename(&tmp, self.entry_path(id))?;
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), Error> {
        match std::fs::remove_file(self.entry_path(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    fn backend_name(&self) -> &'static str {
        "encrypted file"
    }
}

/// Write `bytes` to `path` with mode 0600 (owner read/write only) on
/// Unix; plain write elsewhere.
fn write_mode_0600(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        use std::io::Write;
        file.write_all(bytes)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)?;
        Ok(())
    }
}

/// An in-memory secret store, for tests (and for headless/dev runs
/// that opt into it explicitly — never the app default).
pub struct MemorySecretStore {
    inner: Mutex<HashMap<String, SecretString>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemorySecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, id: &str) -> Result<Option<SecretString>, Error> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| Error::Flow("secret store lock poisoned".into()))?
            .get(id)
            .cloned())
    }

    fn set(&self, id: &str, secret: &SecretString) -> Result<(), Error> {
        self.inner
            .lock()
            .map_err(|_| Error::Flow("secret store lock poisoned".into()))?
            .insert(id.to_string(), secret.clone());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), Error> {
        self.inner
            .lock()
            .map_err(|_| Error::Flow("secret store lock poisoned".into()))?
            .remove(id);
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "memory (tests only)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_file_round_trip() {
        let dir =
            std::env::temp_dir().join(format!("tubelite-secrets-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = EncryptedFileStore::new(&dir);
        let secret = SecretString::new("SAPISID=round-trip-value");
        assert_eq!(store.get("ga-abc").unwrap(), None);
        store.set("ga-abc", &secret).unwrap();
        assert_eq!(store.get("ga-abc").unwrap().unwrap(), secret);
        // The file on disk must not contain the plaintext.
        let on_disk = std::fs::read(store.entry_path("ga-abc")).unwrap();
        assert!(!on_disk.windows(10).any(|w| w == b"round-trip"));
        store.delete("ga-abc").unwrap();
        assert_eq!(store.get("ga-abc").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn encrypted_file_key_is_0600_on_unix() {
        let dir =
            std::env::temp_dir().join(format!("tubelite-secrets-perm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = EncryptedFileStore::new(&dir);
        store
            .set("ga-abc", &SecretString::new("SAPISID=x"))
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store.key_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
            let entry_mode = std::fs::metadata(store.entry_path("ga-abc"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(entry_mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn encrypted_file_wrong_key_is_unreadable() {
        let dir =
            std::env::temp_dir().join(format!("tubelite-secrets-wrongkey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = EncryptedFileStore::new(&dir);
        store
            .set("ga-abc", &SecretString::new("SAPISID=x"))
            .unwrap();
        // Replace the key with a different one: the entry must no
        // longer decrypt, and the error must not leak the secret.
        let mut other_key = [0u8; 32];
        getrandom::fill(&mut other_key).unwrap();
        std::fs::write(store.key_path(), other_key).unwrap();
        let err = store.get("ga-abc").unwrap_err();
        assert!(matches!(err, Error::Crypto(_)));
        assert!(!err.to_string().contains("SAPISID"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keyring_probe_matches_the_backend_it_returns() {
        // The probe must agree with the keyring's own status, and the
        // returned store must round-trip a secret either way. On this
        // VM there is no secret service, so the probe fails and the
        // fallback is what gets exercised; on a machine with a
        // keyring it returns Keyring instead.
        let dir =
            std::env::temp_dir().join(format!("tubelite-secrets-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (store, backend) = open_secret_store(&dir);
        let expected = match keyring::Entry::store_status() {
            Ok(()) => SecretBackend::Keyring,
            Err(_) => SecretBackend::EncryptedFile,
        };
        assert_eq!(backend, expected);
        store
            .set("ga-probe", &SecretString::new("SAPISID=probe"))
            .unwrap();
        assert_eq!(
            store.get("ga-probe").unwrap().unwrap().expose(),
            "SAPISID=probe"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
