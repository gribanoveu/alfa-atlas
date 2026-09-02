//! Encrypted storage for the Jira API token. Mirrors
//! `infra::embedding_credentials_store`'s single-value shape (one token, not
//! a map — there is one configured Jira instance) and its "write-only from
//! the frontend's perspective" contract: `commands::jira` never returns the
//! decrypted token over IPC, only a boolean `has_token` status.
//!
//! No compile-time bundled fallback here, unlike the embedding key: a Jira
//! token identifies a *person*, so there is no sensible app-wide default to
//! bake in.
//!
//! No filesystem-touching tests, matching the sibling stores' convention —
//! `settings_dir()` resolves against the real `~/.atlas`, and the crypto
//! helpers this reuses are already covered in `key_management.rs`.

use std::fs;
use std::path::PathBuf;

use crate::infra::key_management::{
    decrypt_private_key, encrypt_private_key, get_or_create_encryption_key,
};
use crate::infra::settings_store;

const CREDENTIALS_FILE: &str = "jira_credentials.enc";

fn credentials_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(CREDENTIALS_FILE))
}

pub fn save_token(token: &str) -> Result<(), String> {
    let key = get_or_create_encryption_key()?;
    let encrypted = encrypt_private_key(token.as_bytes(), &key)?;

    let path = credentials_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("failed to create settings dir: {e}"))?;
    }
    fs::write(&path, &encrypted).map_err(|e| format!("failed to write Jira credentials: {e}"))?;

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

/// Missing file / stale encryption key / corrupt data all degrade to `None`
/// rather than an error — the caller's next step is the same either way
/// (`JiraError::MissingToken`, "add a token in Settings").
pub fn get_token() -> Option<String> {
    let path = credentials_path().ok()?;
    if !path.exists() {
        return None;
    }
    let encrypted = fs::read(&path).ok()?;
    let key = get_or_create_encryption_key().ok()?;
    let plain = decrypt_private_key(&encrypted, &key).ok()?;
    String::from_utf8(plain).ok()
}

pub fn has_token() -> bool {
    credentials_path().map(|p| p.exists()).unwrap_or(false)
}

/// Idempotent — deleting when nothing is stored is not an error.
pub fn delete_token() -> Result<(), String> {
    let path = credentials_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("failed to remove Jira credentials: {e}"))?;
    }
    Ok(())
}
