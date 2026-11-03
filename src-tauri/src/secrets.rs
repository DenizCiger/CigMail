//! Credential storage: the OS keyring (Secret Service / Keychain / Credential
//! Manager) when available, otherwise a table in the local database so the app
//! still works on systems without a keyring daemon. Mobile builds always use the
//! database, which lives in the app's private storage.

use anyhow::Result;

use crate::db::Db;

#[cfg(desktop)]
const SERVICE: &str = "dev.ciger.cigmail";

#[derive(Clone)]
pub struct Secrets {
    db: Db,
}

impl Secrets {
    pub fn new(db: Db) -> Self {
        Secrets { db }
    }

    #[cfg(desktop)]
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        match keyring::Entry::new(SERVICE, key).and_then(|e| e.get_password()) {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => self.db.fallback_secret(key),
            Err(e) => {
                tracing::debug!("keyring unavailable for {key}: {e}");
                self.db.fallback_secret(key)
            }
        }
    }

    #[cfg(desktop)]
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        match keyring::Entry::new(SERVICE, key).and_then(|e| e.set_password(value)) {
            Ok(()) => self.db.set_fallback_secret(key, None),
            Err(e) => {
                tracing::warn!("keyring unavailable, storing {key} in the local database: {e}");
                self.db.set_fallback_secret(key, Some(value))
            }
        }
    }

    #[cfg(desktop)]
    pub fn delete(&self, key: &str) -> Result<()> {
        if let Ok(e) = keyring::Entry::new(SERVICE, key) {
            let _ = e.delete_credential();
        }
        self.db.set_fallback_secret(key, None)
    }

    #[cfg(mobile)]
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        self.db.fallback_secret(key)
    }

    #[cfg(mobile)]
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.db.set_fallback_secret(key, Some(value))
    }

    #[cfg(mobile)]
    pub fn delete(&self, key: &str) -> Result<()> {
        self.db.set_fallback_secret(key, None)
    }

    pub fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        Ok(self.get(key)?.and_then(|s| serde_json::from_str(&s).ok()))
    }

    pub fn set_json<T: serde::Serialize>(&self, key: &str, v: &T) -> Result<()> {
        self.set(key, &serde_json::to_string(v)?)
    }
}
