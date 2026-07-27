use std::path::Path;

use crate::domain::git::GitError;
use crate::infra::{git_credentials_store, git_repo, key_management};

pub fn clone_repository(url: &str, destination: &str) -> Result<(), String> {
    let dest = Path::new(destination);

    let url_trimmed = url.trim();
    if url_trimmed.is_empty() {
        return Err(GitError::Message("clone URL is empty".into()).to_string());
    }

    let canonical_dest = dest
        .canonicalize()
        .unwrap_or_else(|_| dest.to_path_buf());

    if canonical_dest
        .read_dir()
        .ok()
        .map(|mut d| d.next().is_some())
        .unwrap_or(false)
    {
        return Err(GitError::CloneFailed(
            "destination directory is not empty".into(),
        )
        .to_string());
    }

    let repo_config = git2::Config::open_default().map_err(|e| e.to_string())?;
    let credentials = git_credentials_store::load().map_err(|e| e.to_string())?;
    let app_private_key = key_management::get_decrypted_private_key();

    // If no credentials at all (no stored keys + no app key), return a specific
    // error that the frontend can detect to show the auth-required flow.
    if credentials.ssh_keys.is_empty() && app_private_key.is_none() {
        return Err(
            "no_ssh_credentials: SSH authentication is not configured. Add an SSH key in Settings."
                .into(),
        );
    }

    git_repo::clone_repo(
        url_trimmed,
        &canonical_dest,
        &repo_config,
        &credentials,
        app_private_key.as_deref(),
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("docflow-clone-svc-{prefix}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn clone_repository_empty_url_returns_error() {
        let dir = temp_dir("empty-url");
        let dest = dir.join("dest");
        let err = clone_repository("", dest.to_str().unwrap()).unwrap_err();
        assert!(
            err.contains("clone URL is empty"),
            "expected empty URL error, got: {err}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_repository_whitespace_only_url_returns_error() {
        let dir = temp_dir("ws-url");
        let dest = dir.join("dest");
        let err = clone_repository("   ", dest.to_str().unwrap()).unwrap_err();
        assert!(
            err.contains("clone URL is empty"),
            "expected empty URL error, got: {err}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_repository_non_empty_destination_returns_error() {
        let dir = temp_dir("nonempty");
        let dest = dir.join("dest");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("stale.txt"), "old\n").unwrap();

        let err = clone_repository("ssh://git@bitbucket.company.com/repo.git", dest.to_str().unwrap()).unwrap_err();
        assert!(
            err.contains("not empty"),
            "expected not empty error, got: {err}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_repository_nonexistent_destination_no_error_from_service() {
        // The service itself should not fail because the destination doesn't exist.
        // It will fail deeper in git_repo::clone_repo because the URL is bogus,
        // but the service-layer validation (empty URL, non-empty dir) should pass.
        let dir = temp_dir("noexist");
        let dest = dir.join("nonexistent");
        // dest does not exist
        let err = clone_repository("ssh://invalid-host-that-does-not-resolve.invalid/repo.git", dest.to_str().unwrap()).unwrap_err();
        // Should fail at the clone level, not service validation.
        assert!(
            !err.contains("clone URL is empty") && !err.contains("not empty"),
            "service validation should pass; got: {err}"
        );
        fs::remove_dir_all(&dir).ok();
    }
}
