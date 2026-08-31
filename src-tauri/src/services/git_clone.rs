use std::fs;
use std::path::Path;

use crate::domain::git::{GitError, GitProgressEvent};
use crate::domain::paths;
use crate::infra::{git_credentials_store, git_repo, key_management};

/// Clone `url` into `destination`.
///
/// `is_cancelled` is polled from inside libgit2's callbacks; when it turns
/// true the clone is abandoned and, if this call was what created the
/// destination directory, the partial checkout is removed. Without that
/// cleanup an interrupted attempt leaves a directory holding nothing but
/// `.git`, which then blocks every retry into the same path.
pub fn clone_repository<'a>(
    url: &str,
    destination: &str,
    on_progress: Option<&'a mut dyn FnMut(GitProgressEvent)>,
    is_cancelled: Option<&'a (dyn Fn() -> bool + 'a)>,
) -> Result<(), String> {
    let dest = Path::new(destination);

    let url_trimmed = url.trim();
    if url_trimmed.is_empty() {
        return Err(GitError::Message("clone URL is empty".into()).to_string());
    }

    // A not-yet-existing destination cannot be canonicalized, which is the
    // normal case; when it can be, drop the Windows `\\?\` prefix — libgit2
    // does not understand verbatim paths.
    let canonical_dest = dest
        .canonicalize()
        .map(paths::strip_verbatim)
        .unwrap_or_else(|_| dest.to_path_buf());

    if git_repo::directory_is_non_empty(&canonical_dest) {
        return Err(GitError::CloneFailed(
            "destination directory is not empty".into(),
        )
        .to_string());
    }
    let destination_preexisted = canonical_dest.exists();

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

    let outcome = git_repo::clone_repo(
        url_trimmed,
        &canonical_dest,
        &repo_config,
        &credentials,
        app_private_key.as_deref(),
        on_progress,
        is_cancelled,
    );

    match outcome {
        Ok(_) => Ok(()),
        Err(e) => {
            if !destination_preexisted {
                // Best effort: a directory we could not remove is still better
                // reported as the clone failure the user actually hit.
                let _ = fs::remove_dir_all(&canonical_dest);
            }
            Err(e.to_string())
        }
    }
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
            .join(format!("alfa-atlas-clone-svc-{prefix}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn clone_repository_empty_url_returns_error() {
        let dir = temp_dir("empty-url");
        let dest = dir.join("dest");
        let err = clone_repository("", dest.to_str().unwrap(), None, None).unwrap_err();
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
        let err = clone_repository("   ", dest.to_str().unwrap(), None, None).unwrap_err();
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

        let err = clone_repository("ssh://git@bitbucket.company.com/repo.git", dest.to_str().unwrap(), None, None).unwrap_err();
        assert!(
            err.contains("not empty"),
            "expected not empty error, got: {err}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_repository_removes_the_directory_it_created_on_failure() {
        // The symptom this cleanup exists for: a failed attempt used to leave a
        // directory holding nothing but `.git`, which then blocked every retry.
        let dir = temp_dir("cleanup");
        let dest = dir.join("dest");
        let err = clone_repository(
            "ssh://invalid-host-that-does-not-resolve.invalid/repo.git",
            dest.to_str().unwrap(),
            None,
            None,
        )
        .unwrap_err();
        assert!(!err.is_empty());
        assert!(
            !dest.exists(),
            "a destination this call created must not survive a failed clone"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_repository_keeps_a_destination_it_did_not_create() {
        let dir = temp_dir("keep-dest");
        let dest = dir.join("dest");
        fs::create_dir_all(&dest).unwrap();
        let _ = clone_repository(
            "ssh://invalid-host-that-does-not-resolve.invalid/repo.git",
            dest.to_str().unwrap(),
            None,
            None,
        );
        assert!(
            dest.exists(),
            "a pre-existing destination is the user's, not ours to delete"
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
        let err = clone_repository("ssh://invalid-host-that-does-not-resolve.invalid/repo.git", dest.to_str().unwrap(), None, None).unwrap_err();
        // Should fail at the clone level, not service validation.
        assert!(
            !err.contains("clone URL is empty") && !err.contains("not empty"),
            "service validation should pass; got: {err}"
        );
        fs::remove_dir_all(&dir).ok();
    }
}
