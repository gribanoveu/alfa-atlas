//! Derives a stable identity for a repository so its embeddings index
//! (`infra::index_store`/`infra::vector_store`) can live in a global,
//! per-repository cache (`~/.atlas/embeddings/{repository_id}`, see
//! `commands::embeddings::resolve_index_paths`) instead of inside the repo
//! itself. Identity is the repo's canonical remote URL — deliberately
//! *not* the current revision, so switching branches or committing doesn't
//! change which cache folder a repo maps to; `resolve_index_paths` still
//! records the revision as metadata for that folder, just not as part of
//! its identity.

use std::path::Path;

use git2::Repository;
use sha2::{Digest, Sha256};

use crate::infra::git_repo::default_remote_name;

pub struct RepositoryIdentity {
    /// Normalized remote URL, or `None` if the path isn't a git repository
    /// or has no remotes configured (a purely local repo). Callers fall
    /// back to a persisted per-project UUID in that case — see
    /// `commands::embeddings::local_identity`.
    pub canonical_url: Option<String>,
    /// Full HEAD commit OID, or `None` for a non-repo or an unborn HEAD
    /// (no commits yet). Informational only — not part of `repository_id`.
    pub revision: Option<String>,
}

/// Resolves `repo_root`'s identity. Never fails: any git error (not a
/// repo, no remotes, detached/unborn HEAD) just leaves the corresponding
/// field `None` rather than surfacing an error, since a project not (yet)
/// being a fully-formed git repo is a normal state, not a fault.
pub fn resolve(repo_root: &Path) -> RepositoryIdentity {
    let repo = match Repository::open(repo_root) {
        Ok(repo) => repo,
        Err(_) => {
            return RepositoryIdentity {
                canonical_url: None,
                revision: None,
            }
        }
    };

    let canonical_url = default_remote_name(&repo)
        .ok()
        .and_then(|name| repo.find_remote(&name).ok())
        .and_then(|remote| remote.url().ok().map(canonicalize_remote_url));

    let revision = repo
        .head()
        .ok()
        .and_then(|head| head.target())
        .map(|oid| oid.to_string());

    RepositoryIdentity {
        canonical_url,
        revision,
    }
}

/// Normalizes a git remote URL so equivalent remotes collapse to the same
/// string regardless of protocol — `git@github.com:org/repo.git`,
/// `ssh://git@github.com/org/repo.git`, and `https://github.com/org/repo`
/// all become `github.com/org/repo`. Strips embedded credentials, scheme,
/// a trailing `.git`, and a trailing `/`; lowercases only the host segment
/// (paths on some hosts are case-sensitive, hostnames never are).
fn canonicalize_remote_url(url: &str) -> String {
    let trimmed = url.trim();

    // SCP-like shorthand (`user@host:path`, no `scheme://`) — rewrite to
    // the same `host/path` shape the scheme-prefixed forms end up in below.
    let normalized = if !trimmed.contains("://") && trimmed.contains(':') && trimmed.contains('@')
    {
        let after_at = trimmed.split_once('@').map_or(trimmed, |(_, rest)| rest);
        after_at.replacen(':', "/", 1)
    } else if let Some(idx) = trimmed.find("://") {
        trimmed[idx + 3..].to_string()
    } else {
        trimmed.to_string()
    };

    // Strip any leftover embedded credentials (`user@` or `user:pass@`)
    // from the scheme-prefixed forms (the SCP-like branch above already
    // consumed its `user@`).
    let without_creds = match normalized.split_once('@') {
        Some((_, rest)) => rest,
        None => normalized.as_str(),
    };

    let without_trailing_slash = without_creds.trim_end_matches('/');
    let without_git_suffix = without_trailing_slash
        .strip_suffix(".git")
        .unwrap_or(without_trailing_slash);

    match without_git_suffix.split_once('/') {
        Some((host, path)) => format!("{}/{}", host.to_lowercase(), path),
        None => without_git_suffix.to_lowercase(),
    }
}

/// `repository_id = SHA-256(source)`, hex-encoded. `source` is
/// `RepositoryIdentity::canonical_url` when available, otherwise the
/// caller's persisted fallback identity (see `commands::embeddings::
/// local_identity`) — either way, a stable string that never changes for
/// the same repo across clones/worktrees/commits.
pub fn repository_id(source: &str) -> String {
    let digest = Sha256::digest(source.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn canonicalize_remote_url_collapses_equivalent_forms() {
        let https = canonicalize_remote_url("https://github.com/org/repo.git");
        let ssh_scheme = canonicalize_remote_url("ssh://git@github.com/org/repo.git");
        let scp_like = canonicalize_remote_url("git@github.com:org/repo.git");
        let trailing_slash = canonicalize_remote_url("https://github.com/org/repo/");
        let mixed_case_host = canonicalize_remote_url("https://GitHub.com/org/repo");

        assert_eq!(https, "github.com/org/repo");
        assert_eq!(ssh_scheme, https);
        assert_eq!(scp_like, https);
        assert_eq!(trailing_slash, https);
        assert_eq!(mixed_case_host, https);
    }

    #[test]
    fn repository_id_is_deterministic_and_distinct() {
        let a = repository_id("github.com/org/repo");
        let b = repository_id("github.com/org/repo");
        let c = repository_id("github.com/org/other-repo");

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("alfa-atlas-repo-identity-{name}-{nanos}-{n}"))
    }

    #[test]
    fn resolve_on_a_non_git_directory_returns_no_identity() {
        let dir = fixture_dir("non-git");
        std::fs::create_dir_all(&dir).unwrap();

        let identity = resolve(&dir);
        assert!(identity.canonical_url.is_none());
        assert!(identity.revision.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_on_a_repo_with_origin_returns_canonical_url_and_head() {
        let dir = fixture_dir("with-origin");
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Repository::init(&dir).unwrap();
        repo.remote("origin", "git@github.com:org/repo.git").unwrap();

        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_id).unwrap();
        let commit_oid = repo
            .commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .unwrap();

        let identity = resolve(&dir);
        assert_eq!(identity.canonical_url.as_deref(), Some("github.com/org/repo"));
        assert_eq!(identity.revision.as_deref(), Some(commit_oid.to_string().as_str()));

        std::fs::remove_dir_all(&dir).ok();
    }
}
