//! Encrypted storage for configured LLM providers' API keys. Mirrors
//! `infra::embedding_credentials_store` exactly (same AES-256-GCM key via
//! `key_management::get_or_create_encryption_key`, same "write-only from
//! the frontend's perspective" contract — nothing in `commands::llm` ever
//! returns a decrypted key over IPC, only a boolean `has_api_key` status),
//! with one structural difference: this is keyed by provider id rather
//! than a single global key, since more than one LLM provider can be
//! configured at once (unlike the single global embedding-provider
//! choice). Stored as one encrypted `HashMap<provider_id, api_key>` blob,
//! not one file per provider — simpler file management for what's always a
//! handful of entries.
//!
//! No filesystem-touching tests here, matching `embedding_credentials_store.rs`'s
//! own convention — `settings_dir()` resolves against the real `~/.atlas`,
//! so only the pure crypto helpers this reuses are unit-tested (already
//! covered in `key_management.rs`'s test module).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::infra::key_management::{decrypt_private_key, encrypt_private_key, get_or_create_encryption_key};
use crate::infra::settings_store;

const CREDENTIALS_FILE: &str = "llm_credentials.enc";

fn credentials_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(CREDENTIALS_FILE))
}

/// Missing file / stale key / corrupt data all degrade to an empty map —
/// mirrors `get_api_key`'s `Option`-returning, never-panics contract on the
/// embedding-credentials sibling.
fn load_all() -> HashMap<String, String> {
    let Ok(path) = credentials_path() else {
        return HashMap::new();
    };
    if !path.exists() {
        return HashMap::new();
    }
    let Ok(encrypted) = fs::read(&path) else {
        return HashMap::new();
    };
    let Ok(key) = get_or_create_encryption_key() else {
        return HashMap::new();
    };
    let Ok(plain) = decrypt_private_key(&encrypted, &key) else {
        return HashMap::new();
    };
    serde_json::from_slice(&plain).unwrap_or_default()
}

fn save_all(map: &HashMap<String, String>) -> Result<(), String> {
    let key = get_or_create_encryption_key()?;
    let plain =
        serde_json::to_vec(map).map_err(|e| format!("failed to serialize LLM credentials: {e}"))?;
    let encrypted = encrypt_private_key(&plain, &key)?;

    let path = credentials_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("failed to create settings dir: {e}"))?;
    }
    fs::write(&path, &encrypted).map_err(|e| format!("failed to write LLM credentials: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&path, perms);
        }
    }

    Ok(())
}

/// Read-decrypt-modify-encrypt-write of the whole blob — a last-write-wins
/// race under concurrent calls, the same non-atomic characteristic every
/// other file under `infra::settings_store` (and its embedding-credentials
/// sibling) already has; not a new risk class here.
pub fn save_api_key(provider_id: &str, api_key: &str) -> Result<(), String> {
    let mut map = load_all();
    map.insert(provider_id.to_string(), api_key.to_string());
    save_all(&map)
}

/// Decrypts and returns `provider_id`'s stored API key, if any — for
/// internal use when actually constructing an `LlmProvider`, never
/// returned from an IPC command.
pub fn get_api_key(provider_id: &str) -> Option<String> {
    load_all().remove(provider_id)
}

pub fn has_api_key(provider_id: &str) -> bool {
    load_all().contains_key(provider_id)
}

pub fn delete_api_key(provider_id: &str) -> Result<(), String> {
    let mut map = load_all();
    if map.remove(provider_id).is_some() {
        save_all(&map)?;
    }
    Ok(())
}
