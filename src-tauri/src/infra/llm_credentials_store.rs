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
use std::path::PathBuf;

use crate::infra::secret_store::{self, SecretPurpose};
use crate::infra::settings_store;

const CREDENTIALS_FILE: &str = "llm_credentials.enc";
const PURPOSE: SecretPurpose = SecretPurpose::LlmCredentials;

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
    let Some(plain) = secret_store::read_secret_file(&path, PURPOSE) else {
        return HashMap::new();
    };
    serde_json::from_slice(&plain).unwrap_or_default()
}

fn save_all(map: &HashMap<String, String>) -> Result<(), String> {
    let plain =
        serde_json::to_vec(map).map_err(|e| format!("failed to serialize LLM credentials: {e}"))?;
    secret_store::write_secret_file(&credentials_path()?, PURPOSE, &plain)
}

/// Read-decrypt-modify-encrypt-write of the whole blob — still a
/// last-write-wins race under concurrent calls (one caller's entry can be
/// dropped by another's write), the same characteristic every other file
/// under `infra::settings_store` has. What `secret_store` does guarantee is
/// that no reader ever sees a half-written file, so a lost update is the
/// worst case rather than a corrupt store.
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
