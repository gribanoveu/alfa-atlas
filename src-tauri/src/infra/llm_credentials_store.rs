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
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;

use secrecy::SecretString;
use zeroize::{Zeroize, Zeroizing};

use crate::infra::secret_store::{self, SecretPurpose};
use crate::infra::settings_store;

const CREDENTIALS_FILE: &str = "llm_credentials.enc";
const PURPOSE: SecretPurpose = SecretPurpose::LlmCredentials;

fn credentials_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(CREDENTIALS_FILE))
}

/// The whole decrypted provider→key map, with every key wiped when the map
/// is dropped.
///
/// `serde_json` has to deserialize into plain `String`s, so the API keys do
/// land in ordinary heap allocations on the way in; what this wrapper
/// guarantees is that they do not simply get freed and left behind. Nothing
/// hands this type out — callers get a single `SecretString` — so the
/// window is one function call wide.
#[derive(Default)]
struct Credentials(HashMap<String, String>);

impl Deref for Credentials {
    type Target = HashMap<String, String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Credentials {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for Credentials {
    fn drop(&mut self) {
        for key in self.0.values_mut() {
            key.zeroize();
        }
    }
}

/// Missing file / stale key / corrupt data all degrade to an empty map —
/// mirrors `get_api_key`'s `Option`-returning, never-panics contract on the
/// embedding-credentials sibling.
fn load_all() -> Credentials {
    let Ok(path) = credentials_path() else {
        return Credentials::default();
    };
    let Some(plain) = secret_store::read_secret_file(&path, PURPOSE) else {
        return Credentials::default();
    };
    Credentials(serde_json::from_slice(&plain).unwrap_or_default())
}

fn save_all(map: &Credentials) -> Result<(), String> {
    let plain = Zeroizing::new(
        serde_json::to_vec(&map.0)
            .map_err(|e| format!("failed to serialize LLM credentials: {e}"))?,
    );
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
pub fn get_api_key(provider_id: &str) -> Option<SecretString> {
    let map = load_all();
    map.get(provider_id).map(|key| SecretString::from(key.as_str()))
}

pub fn has_api_key(provider_id: &str) -> bool {
    load_all().contains_key(provider_id)
}

pub fn delete_api_key(provider_id: &str) -> Result<(), String> {
    let mut map = load_all();
    // `remove` moves the key out of the map, past `Credentials`' own
    // wipe-on-drop — so wipe this one by hand.
    if let Some(mut removed) = map.remove(provider_id) {
        removed.zeroize();
        save_all(&map)?;
    }
    Ok(())
}
