use std::path::{Path, PathBuf};

use git2::{
    build::CheckoutBuilder, AnnotatedCommit, Branch, BranchType, Cred, CredentialType,
    FetchOptions, MergeOptions, PushOptions, RemoteCallbacks, Repository, ResetType, Signature,
    Status, StatusOptions, StatusShow,
};

use crate::domain::git::{
    GitBranchInfo, GitCommitSummary, GitCredentials, GitDiffScope, GitError, GitFileDiff,
    GitFileStatus, GitStatusSnapshot, GitSyncStatus, PullMode, SshKeySource,
};

/// Discover the git workdir containing `path`, or return the canonicalized path itself.
pub fn discover_repo_root(path: &Path) -> PathBuf {
    match Repository::discover(path) {
        Ok(repo) => {
            if let Some(workdir) = repo.workdir() {
                workdir
                    .canonicalize()
                    .unwrap_or_else(|_| workdir.to_path_buf())
            } else {
                path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
            }
        }
        Err(_) => path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
    }
}

/// Current branch shorthand, if the path is inside a git repository with a named head.
pub fn current_branch(repo_root: &Path) -> Option<String> {
    let repo = Repository::open(repo_root).ok()?;
    branch_name(&repo)
}

fn branch_name(repo: &Repository) -> Option<String> {
    let head = repo.head().ok()?;
    if head.is_branch() {
        head.shorthand().ok().map(|s| s.to_string())
    } else {
        head.target().map(|oid| {
            let full = oid.to_string();
            full[..7.min(full.len())].to_string()
        })
    }
}

fn open_repo(repo_root: &Path) -> Result<Repository, GitError> {
    if !repo_root.is_dir() {
        return Err(GitError::NotARepository(repo_root.display().to_string()));
    }
    Repository::open(repo_root).map_err(|e| {
        if e.code() == git2::ErrorCode::NotFound {
            GitError::NotARepository(repo_root.display().to_string())
        } else {
            GitError::Open(e)
        }
    })
}

/// Validates that `path` is a safe, relative, non-parent-escaping path before
/// it is used to index into the repo workdir. Rejects:
/// - empty paths
/// - paths rooted with `/` or `\`
/// - `..` components (parent-dir traversal)
/// - Windows drive-letter / UNC prefixes (e.g. `C:\foo`) and any other root component
fn validate_relative_path(path: &str) -> Result<&Path, GitError> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(GitError::InvalidPath(path.to_string()));
    }
    let p = Path::new(trimmed);
    if p.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir
                | std::path::Component::Prefix(_)
                | std::path::Component::RootDir
        )
    }) {
        return Err(GitError::InvalidPath(path.to_string()));
    }
    Ok(p)
}

fn index_status_letter(status: Status) -> Option<&'static str> {
    if status.contains(Status::INDEX_NEW) {
        Some("A")
    } else if status.contains(Status::INDEX_MODIFIED) {
        Some("M")
    } else if status.contains(Status::INDEX_DELETED) {
        Some("D")
    } else if status.contains(Status::INDEX_RENAMED) {
        Some("R")
    } else if status.contains(Status::INDEX_TYPECHANGE) {
        Some("M")
    } else {
        None
    }
}

fn workdir_status_letter(status: Status) -> Option<&'static str> {
    if status.contains(Status::WT_NEW) {
        Some("?")
    } else if status.contains(Status::WT_MODIFIED) {
        Some("M")
    } else if status.contains(Status::WT_DELETED) {
        Some("D")
    } else if status.contains(Status::WT_RENAMED) {
        Some("R")
    } else if status.contains(Status::WT_TYPECHANGE) {
        Some("M")
    } else {
        None
    }
}

fn tracked_workdir_status_letter(status: Status) -> Option<&'static str> {
    if status.contains(Status::WT_NEW) {
        None
    } else if status.contains(Status::WT_MODIFIED) {
        Some("M")
    } else if status.contains(Status::WT_DELETED) {
        Some("D")
    } else if status.contains(Status::WT_RENAMED) {
        Some("R")
    } else if status.contains(Status::WT_TYPECHANGE) {
        Some("M")
    } else {
        None
    }
}

pub fn status(repo_root: &Path) -> Result<GitStatusSnapshot, GitError> {
    let repo = open_repo(repo_root)?;
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .show(StatusShow::IndexAndWorkdir);

    let statuses = repo.statuses(Some(&mut opts)).map_err(GitError::Operation)?;

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();

    for entry in statuses.iter() {
        let Ok(path) = entry.path() else {
            continue;
        };
        let status = entry.status();
        if let Some(letter) = index_status_letter(status) {
            staged.push(GitFileStatus {
                path: path.to_string(),
                status: letter.to_string(),
            });
        }
        if let Some(letter) = workdir_status_letter(status) {
            unstaged.push(GitFileStatus {
                path: path.to_string(),
                status: letter.to_string(),
            });
        }
    }

    staged.sort_by(|a, b| a.path.cmp(&b.path));
    unstaged.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(GitStatusSnapshot {
        staged,
        unstaged,
        branch: branch_name(&repo),
    })
}

pub fn stage_paths(repo_root: &Path, paths: &[String]) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Message("bare repository is not supported".into()))?;
    let mut index = repo.index().map_err(GitError::Operation)?;

    for path in paths {
        let rel = validate_relative_path(path)?;
        let full = workdir.join(rel);
        if full.exists() {
            index.add_path(rel).map_err(GitError::Operation)?;
        } else {
            // Stage deletion.
            index.remove_path(rel).map_err(GitError::Operation)?;
        }
    }
    index.write().map_err(GitError::Operation)?;
    Ok(())
}

pub fn unstage_paths(repo_root: &Path, paths: &[String]) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let mut validated = Vec::new();
    for path in paths {
        validated.push(validate_relative_path(path)?.to_path_buf());
    }

    if repo.head().is_ok() {
        let head_obj = repo
            .head()
            .map_err(GitError::Operation)?
            .peel_to_commit()
            .map_err(GitError::Operation)?
            .into_object();
        repo.reset_default(Some(&head_obj), validated.iter().map(|p| p.as_path()))
            .map_err(GitError::Operation)?;
    } else {
        // No HEAD yet — remove from index only.
        let mut index = repo.index().map_err(GitError::Operation)?;
        for path in &validated {
            let _ = index.remove_path(path);
        }
        index.write().map_err(GitError::Operation)?;
    }
    Ok(())
}

fn has_staged_changes(repo: &Repository) -> Result<bool, GitError> {
    let mut opts = StatusOptions::new();
    opts.show(StatusShow::Index)
        .include_untracked(false)
        .include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts)).map_err(GitError::Operation)?;
    Ok(statuses
        .iter()
        .any(|e| index_status_letter(e.status()).is_some()))
}

fn commit_signature(repo: &Repository) -> Result<Signature<'static>, GitError> {
    match repo.signature() {
        Ok(sig) => {
            let name = sig.name().unwrap_or("").to_string();
            let email = sig.email().unwrap_or("").to_string();
            if name.trim().is_empty() || email.trim().is_empty() {
                return Err(GitError::MissingIdentity);
            }
            Signature::now(&name, &email).map_err(GitError::Operation)
        }
        Err(_) => Err(GitError::MissingIdentity),
    }
}

pub fn commit(repo_root: &Path, message: &str) -> Result<String, GitError> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(GitError::EmptyMessage);
    }

    let repo = open_repo(repo_root)?;
    if !has_staged_changes(&repo)? {
        return Err(GitError::NothingStaged);
    }

    let mut index = repo.index().map_err(GitError::Operation)?;
    let tree_oid = index.write_tree().map_err(GitError::Operation)?;
    let tree = repo.find_tree(tree_oid).map_err(GitError::Operation)?;
    let sig = commit_signature(&repo)?;

    let parent_commit = match repo.head() {
        Ok(head) => Some(head.peel_to_commit().map_err(GitError::Operation)?),
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => None,
        Err(e) => return Err(GitError::Operation(e)),
    };

    let parents: Vec<&git2::Commit> = match &parent_commit {
        Some(c) => vec![c],
        None => vec![],
    };

    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, trimmed, &tree, &parents)
        .map_err(GitError::Operation)?;

    let full = oid.to_string();
    Ok(full[..7.min(full.len())].to_string())
}

pub fn log(repo_root: &Path, limit: usize) -> Result<Vec<GitCommitSummary>, GitError> {
    let repo = open_repo(repo_root)?;
    let mut walk = match repo.revwalk() {
        Ok(w) => w,
        Err(e) => return Err(GitError::Operation(e)),
    };

    match walk.push_head() {
        Ok(()) => {}
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => return Ok(vec![]),
        Err(e) => return Err(GitError::Operation(e)),
    }

    let mut commits = Vec::new();
    for oid_result in walk.take(limit) {
        let oid = oid_result.map_err(GitError::Operation)?;
        let commit = repo.find_commit(oid).map_err(GitError::Operation)?;
        let full = oid.to_string();
        let hash = full[..7.min(full.len())].to_string();
        let message = commit
            .summary()
            .ok()
            .flatten()
            .or_else(|| commit.message().ok())
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        let author = commit.author().name().unwrap_or("").to_string();
        let time = commit.time().seconds();
        commits.push(GitCommitSummary {
            hash,
            message,
            author,
            time,
        });
    }
    Ok(commits)
}

/// Classifies a remote error as an authentication failure using a small set
/// of specific phrases rather than loose substrings (e.g. plain `"auth"`
/// would also match unrelated messages like "invalid author format").
fn map_remote_error(err: git2::Error) -> GitError {
    const AUTH_FAILURE_MARKERS: [&str; 7] = [
        "authentication",
        "credentials",
        "permission denied",
        "could not read username",
        "401 unauthorized",
        "403 forbidden",
        "access denied",
    ];

    let msg = err.message().to_string();
    let lower = msg.to_lowercase();
    let looks_like_auth_failure = AUTH_FAILURE_MARKERS
        .iter()
        .any(|marker| lower.contains(marker));

    if looks_like_auth_failure {
        GitError::Message(format!("authentication failed: {msg}"))
    } else {
        GitError::Operation(err)
    }
}

/// Extracts the host portion from a git remote URL, for SSH-key host
/// matching. Handles both:
/// - the `ssh://user@host[:port]/path` form
/// - the SCP-like `user@host:path` shorthand (no scheme), which is how most
///   GitHub/Bitbucket/GitLab SSH remotes are written in practice — the
///   previous version only handled `ssh://` and silently failed to match a
///   key's configured host for this much more common form.
fn host_from_url(url: &str) -> Option<&str> {
    if let Some(rest) = url.strip_prefix("ssh://") {
        let host = rest.split('/').next()?;
        return host.split(':').next();
    }
    if !url.contains("://") {
        let after_at = url.split('@').nth(1)?;
        let host = after_at.split(':').next()?;
        if !host.is_empty() {
            return Some(host);
        }
    }
    None
}

fn key_matches_host(host: Option<&str>, config: &crate::domain::git::SshKeyConfig) -> bool {
    let Some(host) = host else {
        return false;
    };
    let Some(pattern) = &config.host else {
        return false;
    };
    host.contains(pattern.as_str())
}

/// Builds the error message used when every credential source has been
/// tried and none worked. Kept as a standalone pure function so the
/// formatting can be unit-tested without a real git transport.
fn credentials_exhausted_message(attempts: &[String]) -> String {
    let detail = if attempts.is_empty() {
        "no credential sources were offered for this URL".to_string()
    } else {
        attempts.join("; ")
    };
    format!("no credentials available (tried: {detail})")
}

fn configure_credentials<'a>(
    callbacks: &mut RemoteCallbacks<'a>,
    config: &'a git2::Config,
    credentials: &'a GitCredentials,
    app_private_key: Option<&'a str>,
) {
    callbacks.credentials(move |url, username_from_url, allowed| {
        // Collected as we go so that, if every source fails, the caller gets
        // a concrete reason per attempt instead of a bare "no credentials"
        // — this ends up in GitError::Message via map_remote_error, not in
        // a log, since credential failures are something the UI may need
        // to explain to the user.
        let mut attempts: Vec<String> = Vec::new();

        if allowed.contains(CredentialType::SSH_KEY) {
            let user = username_from_url.unwrap_or("git");

            // 1. Try the app-managed key (highest priority — user explicitly set it up).
            if let Some(key) = app_private_key {
                match Cred::ssh_key_from_memory(user, None::<&str>, key, None) {
                    Ok(cred) => return Ok(cred),
                    Err(e) => attempts.push(format!("app-managed key: {}", e.message())),
                }
            }

            // 2. Try SSH agent as fallback.
            match Cred::ssh_key_from_agent(user) {
                Ok(cred) => return Ok(cred),
                Err(e) => attempts.push(format!("SSH agent: {}", e.message())),
            }

            // 3. Try stored SSH keys matching the URL host first.
            let url_host = host_from_url(url);
            let key_configs: Vec<&crate::domain::git::SshKeyConfig> =
                credentials.ssh_keys.iter().collect();
            // Try host-matching keys first, then all others.
            let mut matching = Vec::new();
            let mut others = Vec::new();
            for kc in &key_configs {
                if key_matches_host(url_host, kc) {
                    matching.push(*kc);
                } else {
                    others.push(*kc);
                }
            }
            for kc in matching.iter().chain(others.iter()) {
                let passphrase = kc.passphrase.as_deref();
                let result = match &kc.source {
                    SshKeySource::KeyContent { private_key } => {
                        Cred::ssh_key_from_memory(user, None::<&str>, private_key, passphrase)
                    }
                    SshKeySource::KeyFile { path } => {
                        Cred::ssh_key(user, None::<&Path>, Path::new(path), passphrase)
                    }
                };
                match result {
                    Ok(cred) => return Ok(cred),
                    Err(e) => {
                        attempts.push(format!("stored key '{}': {}", kc.name, e.message()))
                    }
                }
            }
        }
        if allowed.contains(CredentialType::USER_PASS_PLAINTEXT)
            || allowed.contains(CredentialType::DEFAULT)
        {
            match Cred::credential_helper(config, url, username_from_url) {
                Ok(cred) => return Ok(cred),
                Err(e) => attempts.push(format!("credential helper: {}", e.message())),
            }
        }

        Err(git2::Error::from_str(&credentials_exhausted_message(
            &attempts,
        )))
    });
}

/// Attach a certificate_check callback that accepts host keys on first
/// connection (trust-on-first-use, no pinning against a known_hosts store).
/// That is a deliberate usability tradeoff for now, but it means a
/// network-level MITM could substitute a different host key undetected. If
/// stronger guarantees are needed later, compare the presented key's
/// fingerprint (`cert.as_hostkey().hash_sha256()`) against a persisted
/// per-remote value and only auto-accept on first contact.
fn configure_ssh_transport(callbacks: &mut RemoteCallbacks<'_>, trust_all: bool) {
    if trust_all {
        callbacks.certificate_check(|_cert, _host| Ok(git2::CertificateCheckStatus::CertificateOk));
    } else {
        callbacks.certificate_check(|_cert, _host| Ok(git2::CertificateCheckStatus::CertificatePassthrough));
    }
}

struct UpstreamRef {
    remote_name: String,
    /// Remote-tracking ref name, e.g. `refs/remotes/origin/main`.
    tracking_ref: String,
    /// Branch name on the remote, e.g. `main`.
    remote_branch: String,
}

fn upstream_of_head(repo: &Repository) -> Result<UpstreamRef, GitError> {
    let head = repo.head().map_err(GitError::Operation)?;
    if !head.is_branch() {
        return Err(GitError::Message(
            "detached HEAD: check out a branch before pull/push".into(),
        ));
    }
    let branch_name = head
        .shorthand()
        .map_err(|_| GitError::Message("cannot determine current branch".into()))?
        .to_string();
    let local = repo
        .find_branch(&branch_name, BranchType::Local)
        .map_err(|_| GitError::NoUpstream)?;
    let upstream = local.upstream().map_err(|_| GitError::NoUpstream)?;
    let tracking_ref = upstream
        .get()
        .name()
        .map_err(|_| GitError::NoUpstream)?
        .to_string();
    let remote_buf = repo
        .branch_upstream_remote(&format!("refs/heads/{branch_name}"))
        .map_err(|_| GitError::NoUpstream)?;
    let remote_name = remote_buf
        .as_str()
        .map_err(|_| GitError::NoUpstream)?
        .to_string();
    let remote_branch = tracking_ref
        .strip_prefix(&format!("refs/remotes/{remote_name}/"))
        .unwrap_or(tracking_ref.as_str())
        .to_string();
    Ok(UpstreamRef {
        remote_name,
        tracking_ref,
        remote_branch,
    })
}

fn fetch_upstream<'repo>(
    repo: &'repo Repository,
    upstream: &UpstreamRef,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
) -> Result<AnnotatedCommit<'repo>, GitError> {
    let config = repo.config().map_err(GitError::Operation)?;
    let mut callbacks = RemoteCallbacks::new();
    configure_credentials(&mut callbacks, &config, credentials, app_private_key);
    configure_ssh_transport(&mut callbacks, credentials.trust_all_ssh_host_keys);

    let mut fetch_opts = FetchOptions::new(); // fetch_upstream
    fetch_opts.remote_callbacks(callbacks);

    let mut remote = repo
        .find_remote(&upstream.remote_name)
        .map_err(map_remote_error)?;
    let refspec = format!(
        "+refs/heads/{0}:refs/remotes/{1}/{0}",
        upstream.remote_branch, upstream.remote_name
    );
    remote
        .fetch(&[refspec.as_str()], Some(&mut fetch_opts), None)
        .map_err(map_remote_error)?;

    let reference = repo
        .find_reference(&upstream.tracking_ref)
        .map_err(GitError::Operation)?;
    repo.reference_to_annotated_commit(&reference)
        .map_err(GitError::Operation)
}

fn head_branch_refname(repo: &Repository) -> Result<String, GitError> {
    let head = repo.head().map_err(GitError::Operation)?;
    head.name()
        .map(str::to_string)
        .map_err(|_| GitError::Message("cannot resolve HEAD ref".into()))
}

fn do_merge(repo: &Repository, theirs: &AnnotatedCommit<'_>) -> Result<(), GitError> {
    let (analysis, _) = repo
        .merge_analysis(&[theirs])
        .map_err(GitError::Operation)?;

    if analysis.is_up_to_date() {
        return Ok(());
    }

    if analysis.is_fast_forward() {
        let refname = head_branch_refname(repo)?;
        let mut reference = repo
            .find_reference(&refname)
            .map_err(GitError::Operation)?;
        reference
            .set_target(theirs.id(), "Fast-Forward")
            .map_err(GitError::Operation)?;
        repo.set_head(&refname).map_err(GitError::Operation)?;
        repo.checkout_head(Some(CheckoutBuilder::default().force()))
            .map_err(GitError::Operation)?;
        return Ok(());
    }

    if analysis.is_normal() {
        let mut opts = MergeOptions::new();
        repo.merge(&[theirs], Some(&mut opts), None)
            .map_err(GitError::Operation)?;

        let mut index = repo.index().map_err(GitError::Operation)?;
        if index.has_conflicts() {
            let _ = repo.cleanup_state();
            return Err(GitError::MergeConflict);
        }

        let tree_oid = index.write_tree().map_err(GitError::Operation)?;
        let tree = repo.find_tree(tree_oid).map_err(GitError::Operation)?;
        let sig = commit_signature(repo)?;
        let head = repo
            .head()
            .map_err(GitError::Operation)?
            .peel_to_commit()
            .map_err(GitError::Operation)?;
        let their_commit = repo
            .find_commit(theirs.id())
            .map_err(GitError::Operation)?;
        let msg = format!(
            "Merge remote-tracking branch '{}'",
            theirs.refname().unwrap_or("upstream")
        );
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &msg,
            &tree,
            &[&head, &their_commit],
        )
        .map_err(GitError::Operation)?;
        repo.cleanup_state().map_err(GitError::Operation)?;
        return Ok(());
    }

    Err(GitError::Message(
        "cannot merge: unsupported merge analysis result".into(),
    ))
}

fn do_rebase(repo: &Repository, theirs: &AnnotatedCommit<'_>) -> Result<(), GitError> {
    let head_ann = {
        let head = repo.head().map_err(GitError::Operation)?;
        repo.reference_to_annotated_commit(&head)
            .map_err(GitError::Operation)?
    };

    let mut rebase = repo
        .rebase(Some(&head_ann), None, Some(theirs), None)
        .map_err(GitError::Operation)?;

    let sig = commit_signature(repo)?;
    while let Some(op) = rebase.next() {
        if let Err(e) = op {
            let _ = rebase.abort();
            return Err(GitError::Operation(e));
        }
        let index = repo.index().map_err(|e| {
            let _ = rebase.abort();
            GitError::Operation(e)
        })?;
        if index.has_conflicts() {
            let _ = rebase.abort();
            return Err(GitError::RebaseConflict);
        }
        match rebase.commit(None, &sig, None) {
            Ok(_) => {}
            Err(e) if e.code() == git2::ErrorCode::Applied => {}
            Err(e) if e.code() == git2::ErrorCode::Conflict => {
                let _ = rebase.abort();
                return Err(GitError::RebaseConflict);
            }
            Err(e) => {
                let _ = rebase.abort();
                return Err(GitError::Operation(e));
            }
        }
    }

    rebase.finish(None).map_err(GitError::Operation)?;
    Ok(())
}

pub fn pull(
    repo_root: &Path,
    mode: PullMode,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let upstream = upstream_of_head(&repo)?;
    let theirs = fetch_upstream(&repo, &upstream, credentials, app_private_key)?;
    match mode {
        PullMode::Merge => do_merge(&repo, &theirs),
        PullMode::Rebase => do_rebase(&repo, &theirs),
    }
}

pub fn sync_status(
    repo_root: &Path,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
) -> Result<GitSyncStatus, GitError> {
    let repo = open_repo(repo_root)?;
    let upstream = upstream_of_head(&repo)?;
    let theirs = fetch_upstream(&repo, &upstream, credentials, app_private_key)?;
    let local = repo
        .head()
        .map_err(GitError::Operation)?
        .peel_to_commit()
        .map_err(GitError::Operation)?;
    let (ahead, behind) = repo
        .graph_ahead_behind(local.id(), theirs.id())
        .map_err(GitError::Operation)?;
    Ok(GitSyncStatus { ahead, behind })
}

pub fn reset_to_remote(
    repo_root: &Path,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let upstream = upstream_of_head(&repo)?;
    let theirs = fetch_upstream(&repo, &upstream, credentials, app_private_key)?;
    let commit = repo
        .find_object(theirs.id(), Some(git2::ObjectType::Commit))
        .map_err(GitError::Operation)?;
    repo.reset(
        &commit,
        ResetType::Hard,
        Some(CheckoutBuilder::default().force()),
    )
    .map_err(GitError::Operation)?;
    Ok(())
}

/// Number of leading bytes inspected when sniffing for binary content —
/// mirrors git's own "first 8000 bytes" heuristic instead of scanning the
/// entire (potentially large) blob for a single NUL byte.
const BINARY_SNIFF_LEN: usize = 8000;

fn is_binary_content(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_SNIFF_LEN)].contains(&0)
}

fn blob_to_text(blob: &git2::Blob<'_>) -> (String, bool) {
    let content = blob.content();
    let binary = is_binary_content(content);
    let text = if binary {
        String::new()
    } else {
        String::from_utf8_lossy(content).into_owned()
    };
    (text, binary)
}

fn read_tree_blob(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    path: &Path,
) -> Option<(String, bool)> {
    let entry = tree.get_path(path).ok()?;
    let object = entry.to_object(repo).ok()?;
    let blob = object.as_blob()?;
    Some(blob_to_text(blob))
}

fn read_head_blob(repo: &Repository, path: &Path) -> Option<(String, bool)> {
    let head = repo.head().ok()?;
    let tree = head.peel_to_tree().ok()?;
    read_tree_blob(repo, &tree, path)
}

fn read_index_blob(repo: &Repository, path: &Path) -> Option<(String, bool)> {
    let index = repo.index().ok()?;
    let entry = index.get_path(path, 0)?;
    let blob = repo.find_blob(entry.id).ok()?;
    Some(blob_to_text(&blob))
}

fn read_workdir_text(workdir: &Path, path: &Path) -> Option<(String, bool)> {
    let full = workdir.join(path);
    if !full.exists() {
        return None;
    }
    let bytes = std::fs::read(&full).ok()?;
    let binary = is_binary_content(&bytes);
    let text = if binary {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    Some((text, binary))
}

fn path_is_untracked(repo: &Repository, path: &str) -> Result<bool, GitError> {
    let mut opts = StatusOptions::new();
    opts.pathspec(path)
        .include_untracked(true)
        .recurse_untracked_dirs(false);
    let statuses = repo.statuses(Some(&mut opts)).map_err(GitError::Operation)?;
    Ok(statuses
        .iter()
        .any(|e| e.status().contains(Status::WT_NEW)))
}

pub fn file_diff(
    repo_root: &Path,
    path: &str,
    scope: GitDiffScope,
) -> Result<GitFileDiff, GitError> {
    let rel = validate_relative_path(path)?;
    let repo = open_repo(repo_root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Message("bare repository is not supported".into()))?;

    match scope {
        GitDiffScope::Staged => {
            let (original, orig_bin) =
                read_head_blob(&repo, rel).unwrap_or_else(|| (String::new(), false));
            let (modified, mod_bin) =
                read_index_blob(&repo, rel).unwrap_or((String::new(), false));
            Ok(GitFileDiff {
                original,
                modified,
                original_label: "HEAD".into(),
                modified_label: "Index".into(),
                is_binary: orig_bin || mod_bin,
            })
        }
        GitDiffScope::Unstaged => {
            let in_index = read_index_blob(&repo, rel);
            let (original, orig_bin) = in_index
                .clone()
                .or_else(|| read_head_blob(&repo, rel))
                .unwrap_or_else(|| (String::new(), false));
            let (modified, mod_bin) =
                read_workdir_text(workdir, rel).unwrap_or((String::new(), false));
            let original_label = if in_index.is_some() { "Index" } else { "HEAD" };
            Ok(GitFileDiff {
                original,
                modified,
                original_label: original_label.into(),
                modified_label: "Working tree".into(),
                is_binary: orig_bin || mod_bin,
            })
        }
    }
}

pub fn discard_file_changes(repo_root: &Path, path: &str) -> Result<(), GitError> {
    let rel = validate_relative_path(path)?;
    let repo = open_repo(repo_root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Message("bare repository is not supported".into()))?;

    if path_is_untracked(&repo, path)? {
        let full = workdir.join(rel);
        if full.is_dir() {
            std::fs::remove_dir_all(&full)
                .map_err(|e| GitError::Message(format!("failed to remove directory: {e}")))?;
        } else if full.is_file() {
            std::fs::remove_file(&full)
                .map_err(|e| GitError::Message(format!("failed to remove file: {e}")))?;
        }
        return Ok(());
    }

    if repo.head().is_ok() {
        let head_obj = repo
            .head()
            .map_err(GitError::Operation)?
            .peel_to_commit()
            .map_err(GitError::Operation)?
            .into_object();
        let tree = head_obj.peel_to_tree().map_err(GitError::Operation)?;
        let tree_obj = tree.as_object();
        let mut checkout = CheckoutBuilder::new();
        checkout.force().path(rel);
        repo.checkout_tree(tree_obj, Some(&mut checkout))
            .map_err(GitError::Operation)?;
        repo.reset_default(Some(&head_obj), std::iter::once(rel))
            .map_err(GitError::Operation)?;
    } else {
        let mut index = repo.index().map_err(GitError::Operation)?;
        let _ = index.remove_path(rel);
        index.write().map_err(GitError::Operation)?;
        let full = workdir.join(rel);
        if full.is_dir() {
            std::fs::remove_dir_all(&full)
                .map_err(|e| GitError::Message(format!("failed to remove directory: {e}")))?;
        } else if full.is_file() {
            std::fs::remove_file(&full)
                .map_err(|e| GitError::Message(format!("failed to remove file: {e}")))?;
        }
    }
    Ok(())
}

pub fn push(
    repo_root: &Path,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let upstream = upstream_of_head(&repo)?;
    let head = repo.head().map_err(GitError::Operation)?;
    let local_branch = head
        .shorthand()
        .map_err(|_| GitError::Message("cannot determine current branch".into()))?;

    let config = repo.config().map_err(GitError::Operation)?;
    let mut callbacks = RemoteCallbacks::new();
    configure_credentials(&mut callbacks, &config, credentials, app_private_key);
    configure_ssh_transport(&mut callbacks, credentials.trust_all_ssh_host_keys);

    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(callbacks);

    let mut remote = repo
        .find_remote(&upstream.remote_name)
        .map_err(map_remote_error)?;
    let refspec = format!(
        "refs/heads/{local_branch}:refs/heads/{}",
        upstream.remote_branch
    );
    remote
        .push(&[refspec.as_str()], Some(&mut push_opts))
        .map_err(map_remote_error)?;
    Ok(())
}

fn validate_branch_name(name: &str) -> Result<&str, GitError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(GitError::Message("branch name is empty".into()));
    }
    if trimmed.contains("..")
        || trimmed.starts_with('/')
        || trimmed.ends_with('/')
        || trimmed.ends_with('.')
        || trimmed.contains("//")
        || trimmed.contains('@')
        || trimmed.contains(' ')
    {
        return Err(GitError::Message(format!(
            "invalid branch name: {trimmed}"
        )));
    }
    Ok(trimmed)
}

fn has_tracked_uncommitted_changes(repo: &Repository) -> Result<bool, GitError> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .show(StatusShow::IndexAndWorkdir);
    let statuses = repo.statuses(Some(&mut opts)).map_err(GitError::Operation)?;
    Ok(statuses.iter().any(|e| {
        index_status_letter(e.status()).is_some()
            || tracked_workdir_status_letter(e.status()).is_some()
    }))
}

fn discard_tracked_changes(repo: &Repository) -> Result<(), GitError> {
    let head = repo.head().map_err(GitError::Operation)?;
    let commit = head.peel_to_commit().map_err(GitError::Operation)?;
    repo.reset(
        commit.as_object(),
        ResetType::Hard,
        Some(CheckoutBuilder::new().force()),
    )
    .map_err(GitError::Operation)?;
    Ok(())
}

fn ensure_clean_or_discard(repo: &Repository, discard_changes: bool) -> Result<(), GitError> {
    if !has_tracked_uncommitted_changes(repo)? {
        return Ok(());
    }
    if discard_changes {
        discard_tracked_changes(repo)
    } else {
        Err(GitError::CheckoutBlocked)
    }
}

fn switch_to_branch(repo: &Repository, branch_name: &str) -> Result<(), GitError> {
    let branch = repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|_| GitError::BranchNotFound(branch_name.to_string()))?;
    let commit = branch
        .get()
        .peel_to_commit()
        .map_err(GitError::Operation)?;
    let tree = commit.tree().map_err(GitError::Operation)?;
    repo.checkout_tree(tree.as_object(), Some(&mut CheckoutBuilder::new().force()))
        .map_err(GitError::Operation)?;
    repo.set_head(&format!("refs/heads/{branch_name}"))
        .map_err(GitError::Operation)?;
    Ok(())
}

/// List local branches and remote-tracking branches (e.g. `origin/feature-x`).
/// The symbolic `origin/HEAD` ref is skipped — it's not a real branch.
pub fn list_branches(repo_root: &Path) -> Result<Vec<GitBranchInfo>, GitError> {
    let repo = open_repo(repo_root)?;
    let current = branch_name(&repo);
    let mut out = Vec::new();

    let locals = repo
        .branches(Some(BranchType::Local))
        .map_err(GitError::Operation)?;
    for branch_result in locals {
        let (branch, _) = branch_result.map_err(GitError::Operation)?;
        let name = branch
            .name()
            .map_err(GitError::Operation)?
            .ok_or_else(|| GitError::Message("branch has invalid name".into()))?
            .to_string();
        let behind = branch_behind_count(&repo, &branch);
        out.push(GitBranchInfo {
            is_current: current.as_deref() == Some(name.as_str()),
            is_remote: false,
            behind,
            name,
        });
    }

    let remotes = repo
        .branches(Some(BranchType::Remote))
        .map_err(GitError::Operation)?;
    for branch_result in remotes {
        let (branch, _) = branch_result.map_err(GitError::Operation)?;
        let Some(name) = branch.name().map_err(GitError::Operation)? else {
            continue;
        };
        if name.rsplit('/').next() == Some("HEAD") {
            continue;
        }
        out.push(GitBranchInfo {
            name: name.to_string(),
            is_current: false,
            is_remote: true,
            behind: None,
        });
    }

    out.sort_by(|a, b| a.is_remote.cmp(&b.is_remote).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// Commits present on `branch`'s upstream (remote-tracking ref) that aren't
/// on `branch` yet — i.e. updates not pulled locally. `None` when the branch
/// has no upstream, or either tip can't be resolved (e.g. unborn branch).
/// Purely local: reflects the state as of the last fetch, not live network
/// data — call `fetch_branches` first to refresh it.
fn branch_behind_count(repo: &Repository, branch: &Branch<'_>) -> Option<usize> {
    let upstream = branch.upstream().ok()?;
    let local_oid = branch.get().target()?;
    let upstream_oid = upstream.get().target()?;
    let (_, behind) = repo.graph_ahead_behind(local_oid, upstream_oid).ok()?;
    Some(behind)
}

/// Fetch every branch from every configured remote, updating the local
/// remote-tracking refs (`refs/remotes/<remote>/*`) without touching the
/// working tree or the current branch. Powers the branches panel's "fetch"
/// action and keeps the per-branch `behind` count accurate.
pub fn fetch_branches(
    repo_root: &Path,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let config = repo.config().map_err(GitError::Operation)?;
    let remote_names = repo.remotes().map_err(GitError::Operation)?;

    for name in remote_names.iter() {
        let Ok(Some(name)) = name else { continue };
        let mut callbacks = RemoteCallbacks::new();
        configure_credentials(&mut callbacks, &config, credentials, app_private_key);
        configure_ssh_transport(&mut callbacks, credentials.trust_all_ssh_host_keys);

        let mut fetch_opts = FetchOptions::new();
        fetch_opts.remote_callbacks(callbacks);

        let mut remote = repo.find_remote(name).map_err(map_remote_error)?;
        remote
            .fetch(&[] as &[&str], Some(&mut fetch_opts), None)
            .map_err(map_remote_error)?;
    }
    Ok(())
}

pub fn create_branch(repo_root: &Path, name: &str, discard_changes: bool) -> Result<(), GitError> {
    let name = validate_branch_name(name)?;
    let repo = open_repo(repo_root)?;
    if repo.find_branch(name, BranchType::Local).is_ok() {
        return Err(GitError::BranchAlreadyExists(name.to_string()));
    }
    ensure_clean_or_discard(&repo, discard_changes)?;
    let head = repo.head().map_err(GitError::Operation)?;
    let commit = head.peel_to_commit().map_err(GitError::Operation)?;
    repo.branch(name, &commit, false)
        .map_err(GitError::Operation)?;
    switch_to_branch(&repo, name)
}

pub fn checkout_branch(
    repo_root: &Path,
    name: &str,
    discard_changes: bool,
) -> Result<(), GitError> {
    let name = validate_branch_name(name)?;
    let repo = open_repo(repo_root)?;
    if repo.find_branch(name, BranchType::Local).is_err() {
        return Err(GitError::BranchNotFound(name.to_string()));
    }

    let current = branch_name(&repo);
    if current.as_deref() == Some(name) {
        return Ok(());
    }

    ensure_clean_or_discard(&repo, discard_changes)?;
    switch_to_branch(&repo, name)
}

/// Check out a remote-tracking branch (e.g. `origin/feature-x`). If a local
/// branch with the same short name (`feature-x`) doesn't exist yet, it is
/// created tracking the remote branch; otherwise the existing local branch
/// is checked out as-is (mirrors `git checkout <remote-shorthand>`).
pub fn checkout_remote_branch(
    repo_root: &Path,
    remote_branch_name: &str,
    discard_changes: bool,
) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let remote_branch = repo
        .find_branch(remote_branch_name, BranchType::Remote)
        .map_err(|_| GitError::BranchNotFound(remote_branch_name.to_string()))?;

    let local_name = remote_branch_name
        .split_once('/')
        .map(|(_, rest)| rest)
        .unwrap_or(remote_branch_name);
    let local_name = validate_branch_name(local_name)?.to_string();

    let current = branch_name(&repo);
    if current.as_deref() == Some(local_name.as_str()) {
        return Ok(());
    }

    ensure_clean_or_discard(&repo, discard_changes)?;

    if repo.find_branch(&local_name, BranchType::Local).is_err() {
        let commit = remote_branch
            .get()
            .peel_to_commit()
            .map_err(GitError::Operation)?;
        let mut local_branch = repo
            .branch(&local_name, &commit, false)
            .map_err(GitError::Operation)?;
        local_branch
            .set_upstream(Some(remote_branch_name))
            .map_err(GitError::Operation)?;
    }

    switch_to_branch(&repo, &local_name)
}

pub fn clone_repo(
    url: &str,
    destination: &Path,
    repo_config: &git2::Config,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
) -> Result<Repository, GitError> {
    if destination.exists() {
        return Err(GitError::DestinationExists(
            destination.display().to_string(),
        ));
    }

    let mut callbacks = RemoteCallbacks::new();
    configure_credentials(&mut callbacks, repo_config, credentials, app_private_key);
    configure_ssh_transport(&mut callbacks, credentials.trust_all_ssh_host_keys);

    let mut fetch_opts = FetchOptions::new(); // clone
    fetch_opts.remote_callbacks(callbacks);

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_opts);

    builder
        .clone(url, destination)
        .map_err(|e| GitError::CloneFailed(e.message().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::git::{GitCredentials, SshKeyConfig};
    use std::fs;

    #[test]
    fn validate_relative_path_rejects_windows_drive_prefix() {
        assert!(matches!(
            validate_relative_path(r"C:\Users\eugene\repo\file.txt"),
            Err(GitError::InvalidPath(_))
        ));
    }

    #[test]
    fn validate_relative_path_accepts_normal_relative_path() {
        assert!(validate_relative_path("src/main.rs").is_ok());
    }

    #[test]
    fn host_from_url_ssh_protocol() {
        // host_from_url strips ssh://, splits on /, then splits on : and takes first.
        assert_eq!(
            host_from_url("ssh://git@bitbucket.company.com:7999/project/repo.git"),
            Some("git@bitbucket.company.com")
        );
    }

    #[test]
    fn host_from_url_ssh_protocol_no_port() {
        assert_eq!(
            host_from_url("ssh://git@github.com/org/repo.git"),
            Some("git@github.com")
        );
    }

    #[test]
    fn host_from_url_non_ssh_returns_none() {
        assert_eq!(host_from_url("https://github.com/org/repo.git"), None);
    }

    #[test]
    fn host_from_url_empty_returns_none() {
        assert_eq!(host_from_url(""), None);
    }

    #[test]
    fn host_from_url_no_host_returns_some_empty() {
        // "ssh://" → empty string after stripping prefix
        assert_eq!(host_from_url("ssh://"), Some(""));
    }

    #[test]
    fn host_from_url_scp_like_syntax() {
        // The common `git@host:path` shorthand used by GitHub/Bitbucket/GitLab,
        // which does not use the `ssh://` scheme at all.
        assert_eq!(
            host_from_url("git@github.com:org/repo.git"),
            Some("github.com")
        );
    }

    #[test]
    fn host_from_url_scp_like_syntax_no_at() {
        assert_eq!(host_from_url("github.com:org/repo.git"), None);
    }

    #[test]
    fn key_matches_host_exact_match() {
        let config = SshKeyConfig {
            name: "test".into(),
            host: Some("bitbucket.company.com".into()),
            source: SshKeySource::KeyContent {
                private_key: "key".into(),
            },
            passphrase: None,
        };
        assert!(key_matches_host(
            Some("git@bitbucket.company.com:7999"),
            &config
        ));
    }

    #[test]
    fn key_matches_host_substring_match() {
        let config = SshKeyConfig {
            name: "test".into(),
            host: Some("bitbucket".into()),
            source: SshKeySource::KeyContent {
                private_key: "key".into(),
            },
            passphrase: None,
        };
        assert!(key_matches_host(
            Some("git@bitbucket.company.com:7999"),
            &config
        ));
    }

    #[test]
    fn key_matches_host_scp_syntax_host() {
        let config = SshKeyConfig {
            name: "test".into(),
            host: Some("github.com".into()),
            source: SshKeySource::KeyContent {
                private_key: "key".into(),
            },
            passphrase: None,
        };
        assert!(key_matches_host(
            host_from_url("git@github.com:org/repo.git"),
            &config
        ));
    }

    #[test]
    fn key_matches_host_no_host_in_url() {
        let config = SshKeyConfig {
            name: "test".into(),
            host: Some("bitbucket.company.com".into()),
            source: SshKeySource::KeyContent {
                private_key: "key".into(),
            },
            passphrase: None,
        };
        assert!(!key_matches_host(None, &config));
    }

    #[test]
    fn key_matches_host_no_host_in_config() {
        let config = SshKeyConfig {
            name: "test".into(),
            host: None,
            source: SshKeySource::KeyContent {
                private_key: "key".into(),
            },
            passphrase: None,
        };
        assert!(!key_matches_host(
            Some("git@bitbucket.company.com:7999"),
            &config
        ));
    }

    #[test]
    fn key_matches_host_different_host() {
        let config = SshKeyConfig {
            name: "test".into(),
            host: Some("github.com".into()),
            source: SshKeySource::KeyContent {
                private_key: "key".into(),
            },
            passphrase: None,
        };
        assert!(!key_matches_host(
            Some("git@bitbucket.company.com:7999"),
            &config
        ));
    }

    #[test]
    fn map_remote_error_does_not_misclassify_author_message() {
        // "author" contains "auth" as a substring — this must NOT be treated
        // as an authentication failure.
        let err = git2::Error::from_str("invalid author format");
        match map_remote_error(err) {
            GitError::Operation(_) => {}
            other => panic!("expected GitError::Operation, got {other:?}"),
        }
    }

    #[test]
    fn map_remote_error_classifies_real_auth_failure() {
        let err = git2::Error::from_str("authentication required");
        match map_remote_error(err) {
            GitError::Message(msg) => assert!(msg.contains("authentication failed")),
            other => panic!("expected GitError::Message, got {other:?}"),
        }
    }

    #[test]
    fn credentials_exhausted_message_empty_attempts() {
        let msg = credentials_exhausted_message(&[]);
        assert!(msg.contains("no credential sources were offered"));
    }

    #[test]
    fn credentials_exhausted_message_lists_each_attempt() {
        let attempts = vec![
            "app-managed key: bad passphrase".to_string(),
            "SSH agent: agent not running".to_string(),
            "stored key 'work': permission denied".to_string(),
        ];
        let msg = credentials_exhausted_message(&attempts);
        assert!(msg.contains("app-managed key: bad passphrase"));
        assert!(msg.contains("SSH agent: agent not running"));
        assert!(msg.contains("stored key 'work': permission denied"));
    }

    #[test]
    fn credentials_exhausted_message_classified_as_auth_failure() {
        // The message this produces must still trip map_remote_error's
        // auth-failure detection so callers get GitError::Message, not a
        // raw GitError::Operation.
        let attempts = vec!["credential helper: no helper configured".to_string()];
        let msg = credentials_exhausted_message(&attempts);
        let err = git2::Error::from_str(&msg);
        match map_remote_error(err) {
            GitError::Message(m) => assert!(m.contains("authentication failed")),
            other => panic!("expected GitError::Message, got {other:?}"),
        }
    }

    #[test]
    fn clone_repo_destination_exists() {
        let dir = temp_dir("clone-exists");
        let dest = dir.join("repo");
        fs::create_dir_all(&dest).unwrap();

        let config = git2::Config::open_default().unwrap();
        let creds = GitCredentials::default();
        let result = clone_repo("https://example.com/repo.git", &dest, &config, &creds, None);
        assert!(
            matches!(result, Err(GitError::DestinationExists(_))),
            "expected DestinationExists, but got a different result"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_repo_invalid_url() {
        let dir = temp_dir("clone-invalid-url");
        let dest = dir.join("empty");

        let config = git2::Config::open_default().unwrap();
        let creds = GitCredentials::default();
        let result = clone_repo("not-a-valid-url", &dest, &config, &creds, None);
        assert!(
            result.is_err(),
            "expected error for invalid URL, but clone succeeded"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_repo_local_bare_succeeds() {
        use std::process::Command;

        let base = temp_dir("clone-local");
        let remote = base.join("remote");
        let dest = base.join("dest");

        // Create a bare repo to clone from.
        Repository::init_bare(&remote).unwrap();
        // Init with a commit so there's something there.
        let tmp = temp_dir("clone-tmp");
        {
            let tmp_repo = Repository::init(&tmp).unwrap();
            {
                let mut config = tmp_repo.config().unwrap();
                config.set_str("user.name", "Test").unwrap();
                config.set_str("user.email", "test@test.com").unwrap();
            }
            fs::write(tmp.join("README.md"), "# Hello\n").unwrap();
            let status = Command::new("git")
                .args(["add", "."])
                .current_dir(&tmp)
                .status()
                .unwrap();
            assert!(status.success());
            let status = Command::new("git")
                .args(["commit", "-m", "init"])
                .current_dir(&tmp)
                .status()
                .unwrap();
            assert!(status.success());
            let status = Command::new("git")
                .args(["push", remote.to_str().unwrap(), "HEAD:refs/heads/main"])
                .current_dir(&tmp)
                .status()
                .unwrap();
            assert!(status.success(), "git push failed");
        }
        // Set HEAD in the bare repo so git2 clone knows the default branch.
        {
            let repo = Repository::open(&remote).unwrap();
            repo.set_head("refs/heads/main").unwrap();
        }

        let config = git2::Config::open_default().unwrap();
        let creds = GitCredentials::default();
        let repo = clone_repo(remote.to_str().unwrap(), &dest, &config, &creds, None).unwrap();
        assert!(dest.join("README.md").exists());
        assert!(!repo.is_bare());

        fs::remove_dir_all(&base).ok();
        fs::remove_dir_all(&tmp).ok();
    }

    // Reuse the temp_dir helper from services::git_ops tests.
    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("alfa-atlas-git-{prefix}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}