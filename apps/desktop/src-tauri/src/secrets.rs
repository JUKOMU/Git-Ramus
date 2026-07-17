use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::error::AppError;

pub trait SecretStore: Send + Sync {
    fn set(&self, key: &str, secret: &str) -> Result<(), AppError>;
    fn get(&self, key: &str) -> Result<Option<String>, AppError>;
    fn delete(&self, key: &str) -> Result<(), AppError>;
}

#[derive(Default)]
pub struct MemorySecretStore {
    values: Arc<Mutex<HashMap<String, String>>>,
}

impl SecretStore for MemorySecretStore {
    fn set(&self, key: &str, secret: &str) -> Result<(), AppError> {
        self.values.lock().insert(key.to_owned(), secret.to_owned());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        Ok(self.values.lock().get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<(), AppError> {
        self.values.lock().remove(key);
        Ok(())
    }
}

pub struct KeyringSecretStore {
    service: String,
}

impl KeyringSecretStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::v1::Entry, AppError> {
        keyring::v1::Entry::new(&self.service, key).map_err(|_| AppError::SecretStore)
    }
}

impl SecretStore for KeyringSecretStore {
    fn set(&self, key: &str, secret: &str) -> Result<(), AppError> {
        self.entry(key)?
            .set_password(secret)
            .map_err(|_| AppError::SecretStore)
    }

    fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::v1::Error::NoEntry) => Ok(None),
            Err(_) => Err(AppError::SecretStore),
        }
    }

    fn delete(&self, key: &str) -> Result<(), AppError> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::v1::Error::NoEntry) => Ok(()),
            Err(_) => Err(AppError::SecretStore),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MemorySecretStore, SecretStore};

    #[test]
    fn memory_store_round_trips_and_deletes_a_secret() {
        let store = MemorySecretStore::default();
        store
            .set("provider/account", "secret-value")
            .expect("set works");
        assert_eq!(
            store.get("provider/account").expect("get works"),
            Some("secret-value".to_owned())
        );
        store.delete("provider/account").expect("delete works");
        assert_eq!(store.get("provider/account").expect("get works"), None);
    }
}
