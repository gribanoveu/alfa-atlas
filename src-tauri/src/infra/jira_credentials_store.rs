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

use secrecy::SecretString;

use crate::infra::secret_store::{self, SecretPurpose};
use crate::infra::settings_store;

const CREDENTIALS_FILE: &str = "jira_credentials.enc";
const PURPOSE: SecretPurpose = SecretPurpose::JiraToken;

fn credentials_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(CREDENTIALS_FILE))
}

pub fn save_token(token: &str) -> Result<(), String> {
    secret_store::write_secret_file(&credentials_path()?, PURPOSE, token.as_bytes())
}

/// Missing file / stale master key / corrupt data all degrade to `None`
/// rather than an error — the caller's next step is the same either way
/// (`JiraError::MissingToken`, "add a token in Settings").
///
/// Returns a `SecretString` so the token cannot reach a log, an error
/// message or a crash dump by way of `Debug`/`Serialize`, and is wiped when
/// the caller drops it.
pub fn get_token() -> Option<SecretString> {
    let plain = secret_store::read_secret_file(&credentials_path().ok()?, PURPOSE)?;
    std::str::from_utf8(&plain).ok().map(SecretString::from)
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
