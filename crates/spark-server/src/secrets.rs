//! Provider API keys live in the OS keychain (Keychain, Credential Manager, Secret Service), never
//! in `.spark` files, settings or logs. `SPARK_KEYRING_FILE` points tests at a private JSON file
//! instead, so test runs never touch a real keychain.

use std::{collections::BTreeMap, path::PathBuf};

const SERVICE: &str = "Spark";

fn file_store() -> Option<PathBuf> {
    std::env::var_os("SPARK_KEYRING_FILE").map(PathBuf::from)
}

fn read_file(path: &PathBuf) -> BTreeMap<String, String> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn get(provider: &str) -> Option<String> {
    let user = format!("provider:{provider}");
    if let Some(path) = file_store() {
        return read_file(&path).get(&user).cloned();
    }
    keyring::Entry::new(SERVICE, &user).ok()?.get_password().ok().filter(|k| !k.is_empty())
}

/// Stores a key; an empty key deletes it.
pub fn set(provider: &str, key: &str) -> Result<(), String> {
    let user = format!("provider:{provider}");
    if let Some(path) = file_store() {
        let mut all = read_file(&path);
        if key.is_empty() {
            all.remove(&user);
        } else {
            all.insert(user, key.to_string());
        }
        return crate::config::write_private(&path, &serde_json::to_vec(&all).unwrap()).map_err(|e| e.to_string());
    }
    let entry = keyring::Entry::new(SERVICE, &user).map_err(|e| format!("the system keychain isn't available: {e}"))?;
    if key.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    } else {
        entry.set_password(key).map_err(|e| format!("couldn't save the key in the system keychain: {e}"))
    }
}
