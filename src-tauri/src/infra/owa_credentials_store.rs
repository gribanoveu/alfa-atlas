//! Encrypted storage for the domain password, when the user opts to remember
//! it. Same shape as `jira_credentials_store`, but a distinct
//! `SecretPurpose::CalendarPassword` — the AAD binding means this blob can
//! never be opened as a Jira token or vice versa.
//!
//! A domain password is heavier than a Jira token (see the plan's "Хранение
//! доменного пароля"): remembering it is opt-in, and the panel says plainly
//! what on-disk encryption here does and does not protect against.

use std::path::PathBuf;

use crate::infra::secret_store::{self, SecretPurpose};
use crate::infra::settings_store;

const CREDENTIALS_FILE: &str = "calendar_credentials.enc";
const PURPOSE: SecretPurpose = SecretPurpose::CalendarPassword;

fn credentials_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(CREDENTIALS_FILE))
}

pub fn save_password(password: &str) -> Result<(), String> {
    secret_store::write_secret_file(&credentials_path()?, PURPOSE, password.as_bytes())
}

pub fn get_password() -> Option<String> {
    let plain = secret_store::read_secret_file(&credentials_path().ok()?, PURPOSE)?;
    String::from_utf8(plain.to_vec()).ok()
}

pub fn has_password() -> bool {
    credentials_path().map(|p| p.exists()).unwrap_or(false)
}

/// Idempotent — deleting when nothing is stored is not an error.
pub fn delete_password() -> Result<(), String> {
    let path = credentials_path()?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("failed to remove calendar password: {e}"))?;
    }
    Ok(())
}
