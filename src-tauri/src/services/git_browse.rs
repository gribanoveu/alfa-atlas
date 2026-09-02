//! Browse links for the open repository: read its remote, fold in what the
//! build declares about the forge, hand back a URL.
//!
//! Thin on purpose — the shape of every link lives in `domain::git_browse`,
//! which is pure and tested there. All this layer adds is "which repository"
//! and "which forge this build talks to".
//!
//! The repository is resolved here rather than passed in, the same way
//! `services::artifacts` does it: every caller means the project that is
//! currently open, and threading its root through the chat hook and the
//! editor would only give them a way to disagree about it.

use std::path::Path;

use crate::domain::git_browse::{browse_url, PATH_PLACEHOLDER};
use crate::infra::{llm_provider_manifest, repository_identity};
use crate::services::repository_scope;

/// A browsable link to `path` (repository-relative) in the open repository.
///
/// `None` whenever anything is unknown — no project open, not a repository,
/// no remote, or a host nothing says how to build links for. That is a
/// routine state, not a failure: a local-only repo simply has no web
/// address, and the callers (a menu item, a context line for the model)
/// omit the link rather than showing a broken one.
pub fn url_for(path: &str, branch: Option<&str>) -> Option<String> {
    let (_, repo_root) = repository_scope::open_repository().ok()?;
    url_in(Path::new(&repo_root), path, branch)
}

/// The same link with `{path}` left standing in for the file — what the
/// assistant is handed so it can address any file without a round trip per
/// link. Substitution, not concatenation: the path sits mid-URL on GitHub
/// and GitLab rather than at the end.
pub fn template(branch: Option<&str>) -> Option<String> {
    url_for(PATH_PLACEHOLDER, branch)
}

/// Split out so the repository can be named explicitly in tests, where
/// there is no open project.
fn url_in(repo_root: &Path, path: &str, branch: Option<&str>) -> Option<String> {
    let remote = repository_identity::remote_url(repo_root)?;
    browse_url(&remote, llm_provider_manifest::git_preset(), path, branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whatever this build declares, an address that is not a repository has
    /// no remote to build anything from.
    #[test]
    fn a_path_that_is_not_a_repository_has_no_link() {
        assert_eq!(url_in(Path::new("/nonexistent-repo-path"), "a.txt", None), None);
    }
}
