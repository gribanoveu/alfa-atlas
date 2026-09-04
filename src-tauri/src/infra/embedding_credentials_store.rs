//! Encrypted storage for the remote embedding provider's API key. Sealed
//! by `infra::secret_store` under the app master key, like every other
//! secret in `~/.atlas`, and keeps the "write-only from the frontend's
//! perspective" contract: nothing in `commands::embeddings` ever returns
//! the decrypted key back over IPC, only a boolean "is one set" status
//! (`has_api_key`).
//!
//! When no user key is stored, `get_api_key` falls back to a compile-time
//! key baked in by `build.rs` (`infra::bundled_secrets`) — see
//! `docs/build-secrets.md`.
//!
//! No filesystem-touching tests here, matching `key_management.rs`'s own
//! convention — `settings_dir()` resolves against the real `~/.atlas`, so
//! only the pure crypto helpers it reuses are unit-tested (already covered
//! in `key_management.rs`'s test module).

use std::fs;
use std::path::PathBuf;

use crate::infra::bundled_secrets;
use crate::infra::secret_store::{self, SecretPurpose};
use crate::infra::settings_store;

const CREDENTIALS_FILE: &str = "embedding_credentials.enc";
const PURPOSE: SecretPurpose = SecretPurpose::EmbeddingApiKey;

fn credentials_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(CREDENTIALS_FILE))
}

fn load_user_key() -> Option<String> {
    let plain = secret_store::read_secret_file(&credentials_path().ok()?, PURPOSE)?;
    String::from_utf8(plain).ok()
}

/// Whether *this user* stored a key, as opposed to `has_api_key`, which is
/// also true for a build-time bundled key. The Settings UI needs the
/// difference: only a user key can be replaced or deleted from there.
pub fn has_user_key() -> bool {
    credentials_path().map(|p| p.exists()).unwrap_or(false)
}

pub fn save_api_key(api_key: &str) -> Result<(), String> {
    secret_store::write_secret_file(&credentials_path()?, PURPOSE, api_key.as_bytes())
}

/// User override first, then compile-time bundled key from `build.rs`.
pub fn get_api_key() -> Option<String> {
    load_user_key().or_else(|| bundled_secrets::EMBEDDING_API_KEY.map(str::to_owned))
}

pub fn has_api_key() -> bool {
    has_user_key() || bundled_secrets::EMBEDDING_API_KEY.is_some()
}

pub fn has_bundled_api_key() -> bool {
    bundled_secrets::EMBEDDING_API_KEY.is_some()
}

/// Idempotent — removes only the user-stored override; a compile-time
/// bundled key (if any) remains available via `get_api_key`.
pub fn delete_api_key() -> Result<(), String> {
    let path = credentials_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("failed to remove embedding credentials: {e}"))?;
    }
    Ok(())
}
