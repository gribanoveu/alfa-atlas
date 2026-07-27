use crate::domain::git::{AppKeyStatus, GitCredentials};
use crate::infra::{git_credentials_store, key_management};

pub fn load_credentials() -> Result<GitCredentials, String> {
    git_credentials_store::load().map_err(|e| e.to_string())
}

pub fn save_credentials(credentials: GitCredentials) -> Result<(), String> {
    git_credentials_store::save(&credentials).map_err(|e| e.to_string())
}

pub fn get_app_key_status() -> Result<AppKeyStatus, String> {
    key_management::ensure_app_key_exists()
}
