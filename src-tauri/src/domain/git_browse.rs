//! Turning a git remote into a link a person can open in a browser.
//!
//! Pure — no I/O, no git2. The caller supplies the remote URL it read.
//!
//! Deliberately *not* built on `infra::repository_identity`'s canonical
//! URL: that one exists to be hashed into a cache folder name, so it throws
//! away the scheme and the `.git` suffix and lowercases the host. What is
//! needed here is the opposite — enough of the original to reconstruct a
//! real address.
//!
//! The hard part is that `host/group/repo` looks identical across forges
//! while their browse URLs do not, so guessing from shape alone would
//! produce a plausible link to nowhere. Only two cases are therefore
//! accepted: `github.com`, which is unambiguous by host, and whatever forge
//! the build declares in `system_providers.yaml` for everything else. No
//! declaration and no match means no link, which is the honest outcome.

use serde::Deserialize;

/// Which forge a non-public git host runs, as declared by the build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitForge {
    /// `{base}/projects/{PROJECT}/repos/{repo}/browse/{path}`
    BitbucketServer,
    /// `{base}/{group}/{repo}/-/blob/{branch}/{path}`
    Gitlab,
}

/// Build-time git settings, from the manifest's `git` section. Absent
/// section means links are only built for hosts recognised on their own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GitPreset {
    /// The forge behind the corporate host. Applied to any remote whose
    /// host is not one of the publicly-known ones.
    pub forge: Option<GitForge>,
}

/// Stands in for the file path when a *template* is wanted rather than one
/// link. Literal braces: the value is handed to a model to substitute into,
/// and no forge's URL grammar gives braces a meaning of their own.
pub const PATH_PLACEHOLDER: &str = "{path}";

/// The pieces of a remote URL a browse link is assembled from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteParts {
    host: String,
    /// Everything between host and repository, e.g. the Bitbucket project
    /// key or the GitLab group. May itself contain `/` for nested groups.
    namespace: String,
    repo: String,
}

/// A browsable link to `path` (repository-relative) at `branch`.
///
/// `None` when the remote cannot be parsed, or when nothing says how this
/// host builds links. An empty `path` yields the repository's own page.
pub fn browse_url(
    remote_url: &str,
    preset: &GitPreset,
    path: &str,
    branch: Option<&str>,
) -> Option<String> {
    let parts = parse_remote(remote_url)?;
    let path = path.trim().trim_start_matches('/');

    // `github.com` is recognised without configuration; anything else is
    // whatever the build says it is, and unset means no link at all.
    let forge = if is_github(&parts.host) {
        return Some(github_url(&parts, path, branch));
    } else {
        preset.forge?
    };

    Some(match forge {
        GitForge::BitbucketServer => bitbucket_url(&parts, path, branch),
        GitForge::Gitlab => gitlab_url(&parts, path, branch),
    })
}

fn is_github(host: &str) -> bool {
    host.eq_ignore_ascii_case("github.com") || host.eq_ignore_ascii_case("www.github.com")
}

/// `https://host/projects/PROJECT/repos/repo/browse/path?at=refs/heads/branch`
///
/// The project key is upper-cased: Bitbucket Server keys are upper-case,
/// while the clone URL carries them lower-cased
/// (`ssh://git@host/proj-key/repo.git` ↔
/// `…/projects/PROJ-KEY/repos/repo/browse/…`).
fn bitbucket_url(parts: &RemoteParts, path: &str, branch: Option<&str>) -> String {
    let project = parts.namespace.to_uppercase();
    let mut url = format!(
        "https://{}/projects/{}/repos/{}/browse",
        parts.host, project, parts.repo
    );
    if !path.is_empty() {
        url.push('/');
        url.push_str(path);
    }
    if let Some(branch) = non_empty(branch) {
        url.push_str("?at=refs/heads/");
        url.push_str(branch);
    }
    url
}

/// `https://host/group/repo/-/blob/branch/path`, falling back to the
/// repository page when there is no path to point at.
fn gitlab_url(parts: &RemoteParts, path: &str, branch: Option<&str>) -> String {
    let base = format!("https://{}/{}/{}", parts.host, parts.namespace, parts.repo);
    if path.is_empty() {
        return base;
    }
    let branch = non_empty(branch).unwrap_or("HEAD");
    format!("{base}/-/blob/{branch}/{path}")
}

/// `https://github.com/owner/repo/blob/branch/path`.
fn github_url(parts: &RemoteParts, path: &str, branch: Option<&str>) -> String {
    let base = format!("https://{}/{}/{}", parts.host, parts.namespace, parts.repo);
    if path.is_empty() {
        return base;
    }
    let branch = non_empty(branch).unwrap_or("HEAD");
    format!("{base}/blob/{branch}/{path}")
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// Splits `host`, `namespace` and `repo` out of any remote spelling git
/// accepts: `ssh://git@host/ns/repo.git`, `https://host/ns/repo.git`,
/// `git@host:ns/repo.git`, with or without a port, with or without `.git`.
///
/// A remote with no namespace at all (`https://host/repo.git`) is rejected
/// rather than guessed at: every forge here needs one, and inventing a
/// value would build a link that resolves to the wrong place.
fn parse_remote(remote_url: &str) -> Option<RemoteParts> {
    let trimmed = remote_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    // SCP-like shorthand (`git@host:ns/repo.git`) has no `scheme://` — turn
    // its single `:` separator into the `/` the rest of this function reads.
    let rest = if let Some(idx) = trimmed.find("://") {
        &trimmed[idx + 3..]
    } else if trimmed.contains(':') && trimmed.contains('@') {
        return parse_remote(&trimmed.replacen(':', "/", 1));
    } else {
        trimmed
    };

    // Strip credentials (`user@` or `user:pass@`).
    let rest = match rest.split_once('@') {
        Some((_, after)) => after,
        None => rest,
    };

    let (host_port, path) = rest.split_once('/')?;
    // A port belongs to the clone URL, never to the web one — Bitbucket
    // Server's SSH remotes routinely carry `:7999`.
    let host = host_port.split(':').next()?.to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }

    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    // Bitbucket's HTTPS clone URLs carry an `/scm` prefix that is absent
    // from every browse URL.
    let path = path.strip_prefix("scm/").unwrap_or(path);

    let (namespace, repo) = path.rsplit_once('/')?;
    if namespace.is_empty() || repo.is_empty() {
        return None;
    }

    Some(RemoteParts {
        host,
        namespace: namespace.to_string(),
        repo: repo.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BITBUCKET: GitPreset = GitPreset {
        forge: Some(GitForge::BitbucketServer),
    };

    /// The shape this whole module was built from — a work repository's
    /// remote, and the link that appears in a ticket's «Ссылки» section for
    /// it. The project key is the part worth pinning: it is lower-case in
    /// the clone URL and upper-case in the web one.
    #[test]
    fn builds_a_deep_link_to_a_file() {
        let url = browse_url(
            "ssh://git@git.example.net/team-scope/team-scope-api.git",
            &BITBUCKET,
            "docs/api/method.adoc",
            None,
        );
        assert_eq!(
            url.as_deref(),
            Some(
                "https://git.example.net/projects/TEAM-SCOPE/repos/team-scope-api/browse/docs/api/method.adoc"
            )
        );
    }

    #[test]
    fn bitbucket_pins_the_branch_when_one_is_given() {
        let url = browse_url(
            "ssh://git@git.example.net/proj/repo.git",
            &BITBUCKET,
            "README.md",
            Some("doc/feature"),
        );
        assert_eq!(
            url.as_deref(),
            Some("https://git.example.net/projects/PROJ/repos/repo/browse/README.md?at=refs/heads/doc/feature")
        );
    }

    #[test]
    fn an_empty_path_points_at_the_repository_itself() {
        let url = browse_url("ssh://git@git.example.net/proj/repo.git", &BITBUCKET, "", None);
        assert_eq!(
            url.as_deref(),
            Some("https://git.example.net/projects/PROJ/repos/repo/browse")
        );
    }

    /// Every spelling git itself accepts has to land on the same link —
    /// which one a checkout happens to use is not the user's choice.
    #[test]
    fn every_remote_spelling_resolves_to_the_same_link() {
        for remote in [
            "ssh://git@git.example.net/proj/repo.git",
            "ssh://git@git.example.net:7999/proj/repo.git",
            "https://user@git.example.net/scm/proj/repo.git",
            "git@git.example.net:proj/repo.git",
            "https://git.example.net/scm/proj/repo",
        ] {
            assert_eq!(
                browse_url(remote, &BITBUCKET, "a.txt", None).as_deref(),
                Some("https://git.example.net/projects/PROJ/repos/repo/browse/a.txt"),
                "wrong link for {remote}"
            );
        }
    }

    /// Recognised by host, so it works in a build that declares no forge —
    /// this repository's own remote is a GitHub one.
    #[test]
    fn github_needs_no_declaration() {
        let url = browse_url(
            "https://github.com/owner/repo.git",
            &GitPreset::default(),
            "README.md",
            Some("main"),
        );
        assert_eq!(
            url.as_deref(),
            Some("https://github.com/owner/repo/blob/main/README.md")
        );
    }

    /// `host/group/repo` looks the same on every forge, so an undeclared
    /// host gets no link rather than a plausible one pointing nowhere.
    #[test]
    fn an_undeclared_host_yields_no_link() {
        assert_eq!(
            browse_url(
                "ssh://git@git.example.net/proj/repo.git",
                &GitPreset::default(),
                "a.txt",
                None,
            ),
            None
        );
    }

    #[test]
    fn gitlab_uses_its_own_blob_path() {
        let preset = GitPreset { forge: Some(GitForge::Gitlab) };
        assert_eq!(
            browse_url("git@gl.example.net:group/sub/repo.git", &preset, "a.txt", Some("main"))
                .as_deref(),
            Some("https://gl.example.net/group/sub/repo/-/blob/main/a.txt")
        );
    }

    #[test]
    fn unparseable_remotes_yield_no_link() {
        for remote in ["", "   ", "not a url", "https://git.example.net/repo.git"] {
            assert_eq!(
                browse_url(remote, &BITBUCKET, "a.txt", None),
                None,
                "expected no link for {remote:?}"
            );
        }
    }

    #[test]
    fn a_leading_slash_on_the_path_is_not_doubled() {
        let url = browse_url("ssh://git@git.example.net/proj/repo.git", &BITBUCKET, "/a.txt", None);
        assert!(url.unwrap().ends_with("/browse/a.txt"));
    }
}
