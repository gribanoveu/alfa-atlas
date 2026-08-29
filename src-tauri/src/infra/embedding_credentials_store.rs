//! Encrypted storage for the remote embedding provider's API key. Mirrors
//! `infra::key_management`'s SSH-private-key storage exactly — same
//! AES-256-GCM key (`key_management::get_or_create_encryption_key`), same
//! "write-only from the frontend's perspective" contract: nothing in
//! `commands::embeddings` ever returns the decrypted key back over IPC,
//! only a boolean "is one set" status (`has_api_key`).
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
use crate::infra::key_management::{decrypt_private_key, encrypt_private_key, get_or_create_encryption_key};
use crate::infra::settings_store;

const CREDENTIALS_FILE: &str = "embedding_credentials.enc";

fn credentials_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(CREDENTIALS_FILE))
}

fn load_user_key() -> Option<String> {
    let path = credentials_path().ok()?;
    if !path.exists() {
        return None;
    }
    let encrypted = fs::read(&path).ok()?;
    let key = get_or_create_encryption_key().ok()?;
    let plain = decrypt_private_key(&encrypted, &key).ok()?;
    String::from_utf8(plain).ok()
}

fn has_user_key() -> bool {
    credentials_path().map(|p| p.exists()).unwrap_or(false)
}

pub fn save_api_key(api_key: &str) -> Result<(), String> {
    let key = get_or_create_encryption_key()?;
    let encrypted = encrypt_private_key(api_key.as_bytes(), &key)?;

    let path = credentials_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("failed to create settings dir: {e}"))?;
    }
    fs::write(&path, &encrypted)
        .map_err(|e| format!("failed to write embedding credentials: {e}"))?;

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
