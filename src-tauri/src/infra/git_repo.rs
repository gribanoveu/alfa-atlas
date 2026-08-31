use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use git2::{
    build::CheckoutBuilder, AnnotatedCommit, BlameOptions, Branch, BranchType, Cred,
    CredentialType, Delta, DiffOptions, FetchOptions, IndexEntry, IndexTime, MergeOptions,
    PushOptions, RemoteCallbacks, Repository, RepositoryState, ResetType, Signature,
    StashApplyOptions, StashSaveOptions, Status, StatusOptions, StatusShow,
};

use crate::domain::git::{
    CheckoutOutcome, GitBlameHunk, GitBranchInfo, GitCommitSummary, GitConflictFile,
    GitCredentials, GitDiffScope, GitError, GitFileDiff, GitFileStatus, GitPhase,
    GitProgressEvent,
    GitResetMode, GitStashEntry, GitStashRestoreOutcome, GitStatusSnapshot, GitSyncStatus,
    PullMode, SshKeyConfig, SshKeySource,
};
use crate::domain::paths;

/// `git2::Error` -> `GitError`, in one place rather than at each of the ~140
/// call sites below. `GitError` carries the message as text (see its own doc
/// comment for why), so this is where the flattening happens.
fn op_err(e: git2::Error) -> GitError {
    GitError::Operation(e.to_string())
}

fn open_err(e: git2::Error) -> GitError {
    GitError::Open(e.to_string())
}

/// Discover the git workdir containing `path`, or return the canonicalized path itself.
pub fn discover_repo_root(path: &Path) -> PathBuf {
    // Every `canonicalize` here goes through `strip_verbatim`: on Windows the
    // raw result carries a `\\?\` prefix, and this value becomes the repo root
    // string stored in project.json and shown throughout the UI.
    match Repository::discover(path) {
        Ok(repo) => {
            if let Some(workdir) = repo.workdir() {
                workdir
                    .canonicalize()
                    .map(paths::strip_verbatim)
                    .unwrap_or_else(|_| workdir.to_path_buf())
            } else {
                path.canonicalize()
                    .map(paths::strip_verbatim)
                    .unwrap_or_else(|_| path.to_path_buf())
            }
        }
        Err(_) => path
            .canonicalize()
            .map(paths::strip_verbatim)
            .unwrap_or_else(|_| path.to_path_buf()),
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
            open_err(e)
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
    // `Component::Prefix` only fires when compiled for Windows, so a
    // drive-letter prefix like `C:\foo` would otherwise slip through
    // unrecognized on macOS/Linux, where `Path` treats it as one opaque
    // component. Check for it explicitly so the rejection is platform-independent.
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
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

struct UnpushedStatus {
    has_commits: bool,
    has_upstream: bool,
    ahead: usize,
}

/// Local-only check (no network) for commits on HEAD that haven't been pushed.
/// `has_upstream` is false for a branch that has never been pushed, in which
/// case every commit on it counts as unpushed regardless of `ahead` (which
/// only reflects the upstream comparison and is `0` when there's no upstream).
fn unpushed_status(repo: &Repository) -> UnpushedStatus {
    let empty = UnpushedStatus {
        has_commits: false,
        has_upstream: false,
        ahead: 0,
    };
    let Ok(head) = repo.head() else {
        return empty;
    };
    if !head.is_branch() {
        return empty;
    }
    let Some(local_oid) = head.target() else {
        return empty;
    };
    let Ok(branch_name) = head.shorthand() else {
        return UnpushedStatus {
            has_commits: true,
            ..empty
        };
    };
    let Ok(branch) = repo.find_branch(branch_name, BranchType::Local) else {
        return UnpushedStatus {
            has_commits: true,
            ..empty
        };
    };
    let Ok(upstream) = branch.upstream() else {
        return UnpushedStatus {
            has_commits: true,
            ..empty
        };
    };
    let ahead = upstream
        .get()
        .target()
        .and_then(|upstream_oid| repo.graph_ahead_behind(local_oid, upstream_oid).ok())
        .map(|(ahead, _)| ahead)
        .unwrap_or(0);
    UnpushedStatus {
        has_commits: true,
        has_upstream: true,
        ahead,
    }
}

pub fn status(repo_root: &Path) -> Result<GitStatusSnapshot, GitError> {
    let repo = open_repo(repo_root)?;
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .show(StatusShow::IndexAndWorkdir);

    let statuses = repo.statuses(Some(&mut opts)).map_err(op_err)?;

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut conflicted = Vec::new();

    for entry in statuses.iter() {
        let Ok(path) = entry.path() else {
            continue;
        };
        let status = entry.status();
        if status.contains(Status::CONFLICTED) {
            conflicted.push(GitFileStatus {
                path: path.to_string(),
                status: "U".to_string(),
            });
            continue;
        }
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
    conflicted.sort_by(|a, b| a.path.cmp(&b.path));

    let unpushed = unpushed_status(&repo);

    Ok(GitStatusSnapshot {
        staged,
        unstaged,
        conflicted,
        branch: branch_name(&repo),
        has_commits: unpushed.has_commits,
        has_upstream: unpushed.has_upstream,
        ahead: unpushed.ahead,
        merge_in_progress: repo.state() == RepositoryState::Merge,
    })
}

pub fn stage_paths(repo_root: &Path, paths: &[String]) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Message("bare repository is not supported".into()))?;
    let mut index = repo.index().map_err(op_err)?;

    for path in paths {
        let rel = validate_relative_path(path)?;
        let full = workdir.join(rel);
        if full.exists() {
            index.add_path(rel).map_err(op_err)?;
        } else {
            // Stage deletion.
            index.remove_path(rel).map_err(op_err)?;
        }
    }
    index.write().map_err(op_err)?;
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
            .map_err(op_err)?
            .peel_to_commit()
            .map_err(op_err)?
            .into_object();
        repo.reset_default(Some(&head_obj), validated.iter().map(|p| p.as_path()))
            .map_err(op_err)?;
    } else {
        // No HEAD yet — remove from index only.
        let mut index = repo.index().map_err(op_err)?;
        for path in &validated {
            let _ = index.remove_path(path);
        }
        index.write().map_err(op_err)?;
    }
    Ok(())
}

fn has_staged_changes(repo: &Repository) -> Result<bool, GitError> {
    let mut opts = StatusOptions::new();
    opts.show(StatusShow::Index)
        .include_untracked(false)
        .include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts)).map_err(op_err)?;
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
            Signature::now(&name, &email).map_err(op_err)
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

    let mut index = repo.index().map_err(op_err)?;
    let tree_oid = index.write_tree().map_err(op_err)?;
    let tree = repo.find_tree(tree_oid).map_err(op_err)?;
    let sig = commit_signature(&repo)?;

    let parent_commit = match repo.head() {
        Ok(head) => Some(head.peel_to_commit().map_err(op_err)?),
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => None,
        Err(e) => return Err(op_err(e)),
    };

    let parents: Vec<&git2::Commit> = match &parent_commit {
        Some(c) => vec![c],
        None => vec![],
    };

    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, trimmed, &tree, &parents)
        .map_err(op_err)?;

    let full = oid.to_string();
    Ok(full[..7.min(full.len())].to_string())
}

/// Undoes a `commit()`/`finish_merge()` call by soft-resetting HEAD to the
/// commit's first parent — index and working tree are left untouched, so
/// this is a clean, low-risk undo (staged files go right back to staged).
/// Refuses if HEAD no longer points at `commit_hash` (something else was
/// committed since) rather than guessing which history to rewrite.
/// `commit_hash` is compared against the short (7-char) form, matching
/// what `commit()`/`finish_merge()` already return and what the frontend
/// action log stores.
pub fn undo_commit(repo_root: &Path, commit_hash: &str) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let head = repo
        .head()
        .map_err(op_err)?
        .peel_to_commit()
        .map_err(op_err)?;
    if short_oid(head.id()) != commit_hash {
        return Err(GitError::Message(
            "HEAD изменился с момента коммита — отмена невозможна".into(),
        ));
    }
    let parent = head.parent(0).map_err(op_err)?;
    repo.reset(parent.as_object(), ResetType::Soft, None)
        .map_err(op_err)?;
    Ok(())
}

fn commit_summary_from_oid(repo: &Repository, oid: git2::Oid) -> Result<GitCommitSummary, GitError> {
    let commit = repo.find_commit(oid).map_err(op_err)?;
    Ok(commit_summary_from_commit(&commit))
}

fn commit_summary_from_commit(commit: &git2::Commit<'_>) -> GitCommitSummary {
    let full = commit.id().to_string();
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
    GitCommitSummary {
        hash,
        message,
        author,
        time,
    }
}

fn collect_commits_from_walk(
    repo: &Repository,
    walk: &mut git2::Revwalk<'_>,
    limit: usize,
) -> Result<Vec<GitCommitSummary>, GitError> {
    let mut commits = Vec::new();
    for oid_result in walk.take(limit) {
        let oid = oid_result.map_err(op_err)?;
        commits.push(commit_summary_from_oid(repo, oid)?);
    }
    Ok(commits)
}

pub fn log(repo_root: &Path, limit: usize) -> Result<Vec<GitCommitSummary>, GitError> {
    let repo = open_repo(repo_root)?;
    let mut walk = match repo.revwalk() {
        Ok(w) => w,
        Err(e) => return Err(op_err(e)),
    };

    match walk.push_head() {
        Ok(()) => {}
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => return Ok(vec![]),
        Err(e) => return Err(op_err(e)),
    }

    collect_commits_from_walk(&repo, &mut walk, limit)
}

/// Commits on HEAD that haven't been pushed yet (`upstream..HEAD`).
/// When the branch has no upstream, every commit on it is returned.
pub fn unpushed_commits(repo_root: &Path, limit: usize) -> Result<Vec<GitCommitSummary>, GitError> {
    let repo = open_repo(repo_root)?;
    let head = match repo.head() {
        Ok(h) => h,
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => return Ok(vec![]),
        Err(e) => return Err(op_err(e)),
    };
    if !head.is_branch() {
        return Ok(vec![]);
    }
    let local_oid = match head.target() {
        Some(oid) => oid,
        None => return Ok(vec![]),
    };

    let mut walk = repo.revwalk().map_err(op_err)?;
    walk.push(local_oid).map_err(op_err)?;

    if let Ok(branch_name) = head.shorthand() {
        if let Ok(branch) = repo.find_branch(branch_name, BranchType::Local) {
            if let Ok(upstream) = branch.upstream() {
                if let Some(upstream_oid) = upstream.get().target() {
                    walk.hide(upstream_oid).map_err(op_err)?;
                }
            }
        }
    }

    collect_commits_from_walk(&repo, &mut walk, limit)
}

/// Commits on the remote upstream that aren't on HEAD yet (`HEAD..upstream`).
/// Fetches the upstream ref first so the list reflects the live remote state.
pub fn incoming_commits(
    repo_root: &Path,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
    limit: usize,
) -> Result<Vec<GitCommitSummary>, GitError> {
    let repo = open_repo(repo_root)?;
    let upstream = match upstream_of_head(&repo) {
        Ok(upstream) => upstream,
        Err(GitError::NoUpstream) => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let theirs = fetch_upstream(&repo, &upstream, credentials, app_private_key, None)?;
    let local = repo
        .head()
        .map_err(op_err)?
        .peel_to_commit()
        .map_err(op_err)?;
    let local_oid = local.id();
    let upstream_oid = theirs.id();

    let mut walk = repo.revwalk().map_err(op_err)?;
    walk.push(upstream_oid).map_err(op_err)?;
    walk.hide(local_oid).map_err(op_err)?;

    collect_commits_from_walk(&repo, &mut walk, limit)
}

fn reset_type(mode: GitResetMode) -> ResetType {
    match mode {
        GitResetMode::Soft => ResetType::Soft,
        GitResetMode::Mixed => ResetType::Mixed,
        GitResetMode::Hard => ResetType::Hard,
    }
}

fn local_upstream_oid(repo: &Repository) -> Result<git2::Oid, GitError> {
    let upstream = upstream_of_head(repo)?;
    let reference = repo.find_reference(&upstream.tracking_ref).map_err(op_err)?;
    Ok(reference.peel_to_commit().map_err(op_err)?.id())
}

fn unpushed_oid_walk(repo: &Repository, limit: usize) -> Result<Vec<git2::Oid>, GitError> {
    let head = match repo.head() {
        Ok(h) => h,
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => return Ok(vec![]),
        Err(e) => return Err(op_err(e)),
    };
    if !head.is_branch() {
        return Ok(vec![]);
    }
    let local_oid = match head.target() {
        Some(oid) => oid,
        None => return Ok(vec![]),
    };

    let mut walk = repo.revwalk().map_err(op_err)?;
    walk.push(local_oid).map_err(op_err)?;

    if let Ok(branch_name) = head.shorthand() {
        if let Ok(branch) = repo.find_branch(branch_name, BranchType::Local) {
            if let Ok(upstream) = branch.upstream() {
                if let Some(upstream_oid) = upstream.get().target() {
                    walk.hide(upstream_oid).map_err(op_err)?;
                }
            }
        }
    }

    let mut oids = Vec::new();
    for oid_result in walk.take(limit) {
        oids.push(oid_result.map_err(op_err)?);
    }
    oids.reverse();
    Ok(oids)
}

fn is_commit_unpushed(repo: &Repository, oid: git2::Oid) -> Result<bool, GitError> {
    Ok(unpushed_oid_walk(repo, 100)?.contains(&oid))
}

fn cherry_pick_commit(repo: &Repository, commit: &git2::Commit<'_>) -> Result<(), GitError> {
    repo.cherrypick(commit, None).map_err(op_err)?;
    let mut index = repo.index().map_err(op_err)?;
    if index.has_conflicts() {
        let _ = repo.cleanup_state();
        return Err(GitError::MergeConflict);
    }
    index.write().map_err(op_err)?;
    let tree_oid = index.write_tree().map_err(op_err)?;
    let tree = repo.find_tree(tree_oid).map_err(op_err)?;
    let sig = commit_signature(repo)?;
    let head = repo.head().map_err(op_err)?.peel_to_commit().map_err(op_err)?;
    let message = commit
        .summary()
        .ok()
        .flatten()
        .or_else(|| commit.message().ok())
        .unwrap_or("cherry pick");
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&head])
        .map_err(op_err)?;
    Ok(())
}

/// Removes `commit_hash` and all newer unpushed commits by resetting to the
/// parent of `commit_hash`.
pub fn drop_unpushed_from(
    repo_root: &Path,
    commit_hash: &str,
    mode: GitResetMode,
) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let commit = resolve_commit(&repo, commit_hash)?;
    if !is_commit_unpushed(&repo, commit.id())? {
        return Err(GitError::Message(
            "можно удалять только неотправленные коммиты".into(),
        ));
    }
    let parent = commit.parent(0).map_err(|_| {
        GitError::Message("нельзя удалить единственный коммит в репозитории".into())
    })?;
    repo.reset(parent.as_object(), reset_type(mode), None)
        .map_err(op_err)
}

/// Resets the current branch to its upstream tracking ref, dropping all unpushed commits.
pub fn drop_all_unpushed(repo_root: &Path, mode: GitResetMode) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let upstream_oid = local_upstream_oid(&repo).map_err(|e| match e {
        GitError::NoUpstream => GitError::Message(
            "у ветки нет upstream — удаляйте коммиты по одному".into(),
        ),
        other => other,
    })?;
    let upstream = repo.find_commit(upstream_oid).map_err(op_err)?;
    repo.reset(
        upstream.as_object(),
        reset_type(mode),
        Some(&mut CheckoutBuilder::new().force()),
    )
    .map_err(op_err)
}

/// Creates `new_name` at HEAD, removes unpushed commits from the current branch
/// (when upstream exists), and checks out `new_name`.
pub fn move_unpushed_to_new_branch(repo_root: &Path, new_name: &str) -> Result<(), GitError> {
    let new_name = validate_branch_name(new_name)?;
    let repo = open_repo(repo_root)?;
    if repo.find_branch(new_name, BranchType::Local).is_ok() {
        return Err(GitError::BranchAlreadyExists(new_name.to_string()));
    }
    let current = branch_name(&repo)
        .ok_or_else(|| GitError::Message("detached HEAD".into()))?;
    if current == new_name {
        return Err(GitError::Message("уже на этой ветке".into()));
    }
    if unpushed_oid_walk(&repo, 1)?.is_empty() {
        return Err(GitError::Message("нет неотправленных коммитов".into()));
    }
    let head = repo.head().map_err(op_err)?.peel_to_commit().map_err(op_err)?;
    repo.branch(new_name, &head, false).map_err(op_err)?;

    if local_upstream_oid(&repo).is_ok() {
        drop_all_unpushed(repo_root, GitResetMode::Hard)?;
    }

    switch_to_branch(&open_repo(repo_root)?, new_name)
}

/// Cherry-picks all unpushed commits onto `target_branch`, then removes them from the source branch.
pub fn move_unpushed_to_branch(repo_root: &Path, target_branch: &str) -> Result<(), GitError> {
    let target_branch = validate_branch_name(target_branch)?;
    let repo = open_repo(repo_root)?;
    let source = branch_name(&repo)
        .ok_or_else(|| GitError::Message("detached HEAD".into()))?;
    if source == target_branch {
        return Err(GitError::Message("нельзя перенести на текущую ветку".into()));
    }
    if repo.find_branch(target_branch, BranchType::Local).is_err() {
        return Err(GitError::BranchNotFound(target_branch.to_string()));
    }
    let oids = unpushed_oid_walk(&repo, 100)?;
    if oids.is_empty() {
        return Err(GitError::Message("нет неотправленных коммитов".into()));
    }
    drop(repo);

    checkout_branch(repo_root, target_branch, false)?;
    {
        let repo = open_repo(repo_root)?;
        for oid in oids {
            let commit = repo.find_commit(oid).map_err(op_err)?;
            cherry_pick_commit(&repo, &commit)?;
        }
    }

    checkout_branch(repo_root, &source, false)?;
    drop_all_unpushed(repo_root, GitResetMode::Hard)?;
    checkout_branch(repo_root, target_branch, false)?;
    Ok(())
}

fn short_oid(oid: git2::Oid) -> String {
    let full = oid.to_string();
    full[..7.min(full.len())].to_string()
}

/// Resolve a (possibly abbreviated) commit hash to the commit object.
fn resolve_commit<'repo>(
    repo: &'repo Repository,
    commit_ref: &str,
) -> Result<git2::Commit<'repo>, GitError> {
    let obj = repo
        .revparse_single(commit_ref)
        .map_err(|_| GitError::Message(format!("commit not found: {commit_ref}")))?;
    obj.peel_to_commit().map_err(op_err)
}

fn delta_status_letter(status: Delta) -> Option<&'static str> {
    match status {
        Delta::Added => Some("A"),
        Delta::Deleted => Some("D"),
        Delta::Modified | Delta::Typechange => Some("M"),
        Delta::Renamed | Delta::Copied => Some("R"),
        _ => None,
    }
}

/// Files changed by `commit_ref` relative to its first parent (or, for a
/// root commit, relative to an empty tree).
pub fn commit_files(repo_root: &Path, commit_ref: &str) -> Result<Vec<GitFileStatus>, GitError> {
    let repo = open_repo(repo_root)?;
    let commit = resolve_commit(&repo, commit_ref)?;
    let tree = commit.tree().map_err(op_err)?;
    let parent_tree = match commit.parent(0) {
        Ok(parent) => Some(parent.tree().map_err(op_err)?),
        Err(_) => None,
    };

    let mut diff_opts = DiffOptions::new();
    let diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut diff_opts))
        .map_err(op_err)?;

    let mut files: Vec<GitFileStatus> = diff
        .deltas()
        .filter_map(|delta| {
            let letter = delta_status_letter(delta.status())?;
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())?;
            Some(GitFileStatus {
                path: path.to_string_lossy().into_owned(),
                status: letter.to_string(),
            })
        })
        .collect();

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Diff of a single file between `commit_ref` and its first parent (empty
/// tree for a root commit) — used for the read-only commit-history file view.
pub fn commit_file_diff(
    repo_root: &Path,
    commit_ref: &str,
    path: &str,
) -> Result<GitFileDiff, GitError> {
    let rel = validate_relative_path(path)?;
    let repo = open_repo(repo_root)?;
    let commit = resolve_commit(&repo, commit_ref)?;
    let tree = commit.tree().map_err(op_err)?;
    let parent = commit.parent(0).ok();
    let parent_tree = match &parent {
        Some(p) => Some(p.tree().map_err(op_err)?),
        None => None,
    };

    let (modified, mod_bin) =
        read_tree_blob(&repo, &tree, rel).unwrap_or_else(|| (String::new(), false));
    let (original, orig_bin) = parent_tree
        .as_ref()
        .and_then(|t| read_tree_blob(&repo, t, rel))
        .unwrap_or_else(|| (String::new(), false));

    let modified_label = short_oid(commit.id());
    let original_label = parent
        .map(|p| short_oid(p.id()))
        .unwrap_or_else(|| "(empty)".into());

    Ok(GitFileDiff {
        original,
        modified,
        original_label,
        modified_label,
        is_binary: orig_bin || mod_bin,
    })
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
        op_err(err)
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

/// Shared control surface for one network git operation (fetch/push/clone):
/// the op name its progress events are tagged with, where those events go,
/// and whether the caller has asked to stop.
///
/// libgit2 invokes every callback through a shared `&` borrow, so the sink
/// lives behind a `RefCell` rather than being handed to a single callback as
/// `&mut` — that is what lets the credentials, host-key, sideband, transfer
/// and checkout callbacks all report into the same channel.
struct NetOpControl<'s> {
    op: RefCell<String>,
    sink: RefCell<Option<&'s mut dyn FnMut(GitProgressEvent)>>,
    cancelled: Option<&'s dyn Fn() -> bool>,
}

impl<'s> NetOpControl<'s> {
    fn new(
        op: impl Into<String>,
        sink: Option<&'s mut dyn FnMut(GitProgressEvent)>,
        cancelled: Option<&'s dyn Fn() -> bool>,
    ) -> Self {
        Self {
            op: RefCell::new(op.into()),
            sink: RefCell::new(sink),
            cancelled,
        }
    }

    fn op(&self) -> String {
        self.op.borrow().clone()
    }

    /// Retag subsequent events. `fetch_branches` reuses one control across
    /// several remotes; re-borrowing the sink per remote is not possible
    /// because `&mut dyn FnMut` is invariant in its own lifetime.
    fn set_op(&self, op: impl Into<String>) {
        *self.op.borrow_mut() = op.into();
    }

    fn emit(&self, event: GitProgressEvent) {
        // `try_borrow_mut` rather than `borrow_mut`: a reentrant callback
        // would otherwise panic inside libgit2's C stack. Dropping a progress
        // update is always preferable to that.
        if let Ok(mut slot) = self.sink.try_borrow_mut() {
            if let Some(sink) = slot.as_mut() {
                sink(event);
            }
        }
    }

    fn phase(&self, phase: GitPhase, detail: Option<String>) {
        self.emit(GitProgressEvent::Phase {
            op: self.op(),
            phase,
            detail,
        });
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.map(|f| f()).unwrap_or(false)
    }

    /// The error every callback returns once the user has cancelled. libgit2
    /// propagates it out of the clone/fetch as a normal failure.
    fn cancel_error() -> git2::Error {
        git2::Error::from_str(CANCELLED_MESSAGE)
    }
}

/// Marker text carried by the error a cancelled operation fails with, so the
/// command layer can tell a deliberate stop from a real failure.
pub const CANCELLED_MESSAGE: &str = "operation cancelled by user";

/// One place credentials can come from, in the order they get tried. Built
/// per credentials callback invocation and filtered against what has already
/// been offered, so libgit2 never receives the same credential twice — doing
/// so makes it retry the same rejected key indefinitely.
enum CredSource<'a> {
    AppKey(&'a str),
    Agent,
    Stored(&'a SshKeyConfig),
    Helper,
}

impl CredSource<'_> {
    /// Stable id used to remember that this source has already been offered.
    fn id(&self) -> String {
        match self {
            CredSource::AppKey(_) => "app-managed key".to_string(),
            CredSource::Agent => "SSH agent".to_string(),
            CredSource::Stored(kc) => format!("stored key '{}'", kc.name),
            CredSource::Helper => "credential helper".to_string(),
        }
    }
}

fn configure_credentials<'a, 's: 'a>(
    callbacks: &mut RemoteCallbacks<'a>,
    config: &'a git2::Config,
    credentials: &'a GitCredentials,
    app_private_key: Option<&'a str>,
    control: &'a NetOpControl<'s>,
) {
    // Sources already offered to libgit2. It calls this back after each
    // rejection, and handing it the same key again is an infinite retry loop
    // rather than a failure the user ever sees.
    let mut offered: HashSet<String> = HashSet::new();
    // Collected as we go so that, if every source fails, the caller gets
    // a concrete reason per attempt instead of a bare "no credentials"
    // — this ends up in GitError::Message via map_remote_error, not in
    // a log, since credential failures are something the UI may need
    // to explain to the user.
    let mut attempts: Vec<String> = Vec::new();

    callbacks.credentials(move |url, username_from_url, allowed| {
        if control.is_cancelled() {
            return Err(NetOpControl::cancel_error());
        }

        for source in credential_sources(url, allowed, credentials, app_private_key) {
            let id = source.id();
            if !offered.insert(id.clone()) {
                continue;
            }
            control.phase(GitPhase::Authenticating, Some(id.clone()));

            let user = username_from_url.unwrap_or("git");
            let result = match source {
                CredSource::AppKey(key) => {
                    Cred::ssh_key_from_memory(user, None::<&str>, key, None)
                }
                CredSource::Agent => Cred::ssh_key_from_agent(user),
                CredSource::Stored(kc) => {
                    let passphrase = kc.passphrase.as_deref();
                    match &kc.source {
                        SshKeySource::KeyContent { private_key } => {
                            Cred::ssh_key_from_memory(user, None::<&str>, private_key.as_str(), passphrase)
                        }
                        SshKeySource::KeyFile { path } => {
                            Cred::ssh_key(user, None::<&Path>, Path::new(path.as_str()), passphrase)
                        }
                    }
                }
                CredSource::Helper => Cred::credential_helper(config, url, username_from_url),
            };

            match result {
                Ok(cred) => return Ok(cred),
                Err(e) => attempts.push(format!("{id}: {}", e.message())),
            }
        }

        Err(git2::Error::from_str(&credentials_exhausted_message(
            &attempts,
        )))
    });
}

/// The credential sources to try for `url`, most-specific first.
///
/// The SSH agent is deliberately last and only when nothing else is
/// configured: on Windows libssh2 reaches Pageant through a blocking
/// `SendMessage` with no timeout, so a stale agent window hangs the whole
/// clone with no way out. A user who has set up a key in the app should never
/// touch that path.
fn credential_sources<'a>(
    url: &str,
    allowed: CredentialType,
    credentials: &'a GitCredentials,
    app_private_key: Option<&'a str>,
) -> Vec<CredSource<'a>> {
    let mut sources = Vec::new();

    if allowed.contains(CredentialType::SSH_KEY) {
        if let Some(key) = app_private_key {
            sources.push(CredSource::AppKey(key));
        }

        // Host-matching keys first, then the rest.
        let url_host = host_from_url(url);
        let (matching, others): (Vec<_>, Vec<_>) = credentials
            .ssh_keys
            .iter()
            .partition(|kc| key_matches_host(url_host, kc));
        sources.extend(matching.into_iter().chain(others).map(CredSource::Stored));

        if sources.is_empty() {
            sources.push(CredSource::Agent);
        }
    }

    if allowed.contains(CredentialType::USER_PASS_PLAINTEXT)
        || allowed.contains(CredentialType::DEFAULT)
    {
        sources.push(CredSource::Helper);
    }

    sources
}

/// Attach a certificate_check callback that accepts host keys on first
/// connection (trust-on-first-use, no pinning against a known_hosts store).
/// That is a deliberate usability tradeoff for now, but it means a
/// network-level MITM could substitute a different host key undetected. If
/// stronger guarantees are needed later, compare the presented key's
/// fingerprint (`cert.as_hostkey().hash_sha256()`) against a persisted
/// per-remote value and only auto-accept on first contact.
fn configure_ssh_transport<'a, 's: 'a>(
    callbacks: &mut RemoteCallbacks<'a>,
    trust_all: bool,
    control: &'a NetOpControl<'s>,
) {
    callbacks.certificate_check(move |_cert, host| {
        if control.is_cancelled() {
            return Err(NetOpControl::cancel_error());
        }
        control.phase(GitPhase::HostKey, Some(host.to_string()));
        Ok(if trust_all {
            git2::CertificateCheckStatus::CertificateOk
        } else {
            git2::CertificateCheckStatus::CertificatePassthrough
        })
    });
}

/// Surfaces the remote's own progress text ("Counting objects…") and, more
/// importantly, gives the connect/negotiate phase a heartbeat: without it a
/// stall before the first byte transfers is indistinguishable from a freeze.
fn configure_sideband<'a, 's: 'a>(
    callbacks: &mut RemoteCallbacks<'a>,
    control: &'a NetOpControl<'s>,
) {
    callbacks.sideband_progress(move |data| {
        if control.is_cancelled() {
            return false;
        }
        let text = String::from_utf8_lossy(data).trim().to_string();
        if !text.is_empty() {
            control.phase(GitPhase::Remote, Some(text));
        }
        true
    });
}

/// Wires `on_progress` into `callbacks.transfer_progress`, used by
/// fetch/pull/clone. Emits one `GitProgressEvent::Transfer` per libgit2
/// progress tick — the caller (`commands/git.rs`) is responsible for
/// throttling before turning these into UI updates.
fn configure_transfer_progress<'a, 's: 'a>(
    callbacks: &mut RemoteCallbacks<'a>,
    control: &'a NetOpControl<'s>,
) {
    callbacks.transfer_progress(move |progress| {
        control.emit(GitProgressEvent::Transfer {
            op: control.op(),
            received_objects: progress.received_objects(),
            total_objects: progress.total_objects(),
            received_bytes: progress.received_bytes(),
            indexed_deltas: progress.indexed_deltas(),
            total_deltas: progress.total_deltas(),
        });
        // Returning false is libgit2's documented way to abort a transfer.
        !control.is_cancelled()
    });
}

/// Wires `on_progress` into `callbacks.push_transfer_progress`, used by push.
fn configure_push_progress<'a, 's: 'a>(
    callbacks: &mut RemoteCallbacks<'a>,
    control: &'a NetOpControl<'s>,
) {
    callbacks.push_transfer_progress(move |current, total, bytes| {
        control.emit(GitProgressEvent::Push {
            op: control.op(),
            current,
            total,
            bytes,
        });
    });
}

struct UpstreamRef {
    remote_name: String,
    /// Remote-tracking ref name, e.g. `refs/remotes/origin/main`.
    tracking_ref: String,
    /// Branch name on the remote, e.g. `main`.
    remote_branch: String,
}

fn upstream_of_head(repo: &Repository) -> Result<UpstreamRef, GitError> {
    let head = repo.head().map_err(op_err)?;
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
    on_progress: Option<&mut dyn FnMut(GitProgressEvent)>,
) -> Result<AnnotatedCommit<'repo>, GitError> {
    let config = repo.config().map_err(op_err)?;
    let control = NetOpControl::new("fetch", on_progress, None);
    let mut callbacks = RemoteCallbacks::new();
    configure_credentials(&mut callbacks, &config, credentials, app_private_key, &control);
    configure_ssh_transport(&mut callbacks, credentials.trust_all_ssh_host_keys, &control);
    configure_sideband(&mut callbacks, &control);
    configure_transfer_progress(&mut callbacks, &control);

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
        .map_err(op_err)?;
    repo.reference_to_annotated_commit(&reference)
        .map_err(op_err)
}

fn head_branch_refname(repo: &Repository) -> Result<String, GitError> {
    let head = repo.head().map_err(op_err)?;
    head.name()
        .map(str::to_string)
        .map_err(|_| GitError::Message("cannot resolve HEAD ref".into()))
}

fn do_merge(repo: &Repository, theirs: &AnnotatedCommit<'_>) -> Result<(), GitError> {
    let (analysis, _) = repo
        .merge_analysis(&[theirs])
        .map_err(op_err)?;

    if analysis.is_up_to_date() {
        return Ok(());
    }

    if analysis.is_fast_forward() {
        let refname = head_branch_refname(repo)?;
        let mut reference = repo
            .find_reference(&refname)
            .map_err(op_err)?;
        reference
            .set_target(theirs.id(), "Fast-Forward")
            .map_err(op_err)?;
        repo.set_head(&refname).map_err(op_err)?;
        repo.checkout_head(Some(CheckoutBuilder::default().force()))
            .map_err(op_err)?;
        return Ok(());
    }

    if analysis.is_normal() {
        let mut opts = MergeOptions::new();
        repo.merge(&[theirs], Some(&mut opts), None)
            .map_err(op_err)?;

        let mut index = repo.index().map_err(op_err)?;
        if index.has_conflicts() {
            // Leave MERGE_HEAD/MERGE_MSG and the conflict-marked working tree
            // files in place — the caller resolves conflicts and finishes
            // (or aborts) the merge explicitly instead of us reverting here.
            return Err(GitError::MergeConflict);
        }

        let tree_oid = index.write_tree().map_err(op_err)?;
        let tree = repo.find_tree(tree_oid).map_err(op_err)?;
        let sig = commit_signature(repo)?;
        let head = repo
            .head()
            .map_err(op_err)?
            .peel_to_commit()
            .map_err(op_err)?;
        let their_commit = repo
            .find_commit(theirs.id())
            .map_err(op_err)?;
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
        .map_err(op_err)?;
        repo.cleanup_state().map_err(op_err)?;
        return Ok(());
    }

    Err(GitError::Message(
        "cannot merge: unsupported merge analysis result".into(),
    ))
}

fn do_rebase(repo: &Repository, theirs: &AnnotatedCommit<'_>) -> Result<(), GitError> {
    let head_ann = {
        let head = repo.head().map_err(op_err)?;
        repo.reference_to_annotated_commit(&head)
            .map_err(op_err)?
    };

    let mut rebase = repo
        .rebase(Some(&head_ann), None, Some(theirs), None)
        .map_err(op_err)?;

    let sig = commit_signature(repo)?;
    while let Some(op) = rebase.next() {
        if let Err(e) = op {
            let _ = rebase.abort();
            return Err(op_err(e));
        }
        let index = repo.index().map_err(|e| {
            let _ = rebase.abort();
            op_err(e)
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
                return Err(op_err(e));
            }
        }
    }

    rebase.finish(None).map_err(op_err)?;
    Ok(())
}

pub fn pull(
    repo_root: &Path,
    mode: PullMode,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
    on_progress: Option<&mut dyn FnMut(GitProgressEvent)>,
) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let upstream = upstream_of_head(&repo)?;
    let theirs = fetch_upstream(&repo, &upstream, credentials, app_private_key, on_progress)?;
    match mode {
        PullMode::Merge => do_merge(&repo, &theirs),
        PullMode::Rebase => do_rebase(&repo, &theirs),
    }
}

/// Reads the current on-disk content (with `<<<<<<<`/`=======`/`>>>>>>>`
/// markers already written by the failed merge's checkout) for a conflicted
/// path, for display in a resolution editor.
pub fn conflict_file_content(repo_root: &Path, path: &str) -> Result<GitConflictFile, GitError> {
    let rel = validate_relative_path(path)?;
    let repo = open_repo(repo_root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Message("bare repository is not supported".into()))?;
    let (content, _binary) = read_workdir_text(workdir, rel)
        .ok_or_else(|| GitError::Message(format!("файл не найден: {path}")))?;
    Ok(GitConflictFile {
        path: path.to_string(),
        content,
    })
}

fn contains_conflict_markers(content: &str) -> bool {
    content.lines().any(|line| {
        line.starts_with("<<<<<<< ")
            || line == "<<<<<<<"
            || line.starts_with(">>>>>>> ")
            || line == ">>>>>>>"
            || line == "======="
    })
}

/// Marks a conflicted path resolved: writes `content` to the working tree
/// and stages it (removing the index's conflict stages). Rejects content
/// that still contains conflict markers so a half-resolved file can't be
/// silently committed.
pub fn resolve_conflict(repo_root: &Path, path: &str, content: &str) -> Result<(), GitError> {
    let rel = validate_relative_path(path)?;
    if contains_conflict_markers(content) {
        return Err(GitError::Message(
            "в файле остались маркеры конфликта — разрешите все блоки перед сохранением".into(),
        ));
    }

    let repo = open_repo(repo_root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Message("bare repository is not supported".into()))?;
    let full = workdir.join(rel);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| GitError::Message(format!("failed to create directory: {e}")))?;
    }
    std::fs::write(&full, content)
        .map_err(|e| GitError::Message(format!("failed to write file: {e}")))?;

    let mut index = repo.index().map_err(op_err)?;
    index.add_path(rel).map_err(op_err)?;
    index.write().map_err(op_err)?;
    Ok(())
}

/// Creates the merge commit once every conflict has been resolved, using
/// the same two parents (HEAD + MERGE_HEAD) and default message a normal
/// `git merge` would have used, then clears the merge state.
pub fn finish_merge(repo_root: &Path) -> Result<String, GitError> {
    let mut repo = open_repo(repo_root)?;
    if repo.state() != RepositoryState::Merge {
        return Err(GitError::Message("нет активного слияния".into()));
    }

    let mut index = repo.index().map_err(op_err)?;
    if index.has_conflicts() {
        let remaining: Vec<String> = index
            .conflicts()
            .map_err(op_err)?
            .filter_map(|c| c.ok())
            .filter_map(|c| {
                c.our
                    .or(c.their)
                    .or(c.ancestor)
                    .and_then(|e| String::from_utf8(e.path).ok())
            })
            .collect();
        return Err(GitError::Message(format!(
            "остались неразрешённые конфликты: {}",
            remaining.join(", ")
        )));
    }

    let mut their_oids = Vec::new();
    repo.mergehead_foreach(|oid| {
        their_oids.push(*oid);
        true
    })
    .map_err(op_err)?;
    let their_oid = *their_oids
        .first()
        .ok_or_else(|| GitError::Message("MERGE_HEAD пуст".into()))?;

    let tree_oid = index.write_tree().map_err(op_err)?;
    let tree = repo.find_tree(tree_oid).map_err(op_err)?;
    let sig = commit_signature(&repo)?;
    let head_commit = repo
        .head()
        .map_err(op_err)?
        .peel_to_commit()
        .map_err(op_err)?;
    let their_commit = repo.find_commit(their_oid).map_err(op_err)?;

    let message = repo
        .message()
        .unwrap_or_else(|_| "Merge remote-tracking branch".into());

    let oid = repo
        .commit(
            Some("HEAD"),
            &sig,
            &sig,
            message.trim(),
            &tree,
            &[&head_commit, &their_commit],
        )
        .map_err(op_err)?;
    repo.cleanup_state().map_err(op_err)?;

    let full = oid.to_string();
    Ok(full[..7.min(full.len())].to_string())
}

/// Abandons an in-progress merge: resets the index and working tree back to
/// HEAD (discarding conflict-marked files) and clears MERGE_HEAD/MERGE_MSG.
/// Equivalent to `git merge --abort`.
///
/// Also accepts the case where the index holds conflicted entries without a
/// formal `RepositoryState::Merge` (no MERGE_HEAD) — e.g. an interrupted
/// merge — since that leaves the user with no other way to discard the
/// conflict from the app.
pub fn abort_merge(repo_root: &Path) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let has_conflicts = repo
        .index()
        .map_err(op_err)?
        .has_conflicts();
    if repo.state() != RepositoryState::Merge && !has_conflicts {
        return Err(GitError::Message("нет активного слияния".into()));
    }
    let head = repo
        .head()
        .map_err(op_err)?
        .peel_to_commit()
        .map_err(op_err)?;
    let mut checkout = CheckoutBuilder::new();
    checkout.force();
    repo.reset(head.as_object(), ResetType::Hard, Some(&mut checkout))
        .map_err(op_err)?;
    repo.cleanup_state().map_err(op_err)?;
    Ok(())
}

pub fn sync_status(
    repo_root: &Path,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
) -> Result<GitSyncStatus, GitError> {
    let repo = open_repo(repo_root)?;
    let upstream = match upstream_of_head(&repo) {
        Ok(upstream) => upstream,
        // No upstream yet (e.g. a brand-new local branch) — nothing to be behind on.
        Err(GitError::NoUpstream) => return Ok(GitSyncStatus { ahead: 0, behind: 0 }),
        Err(e) => return Err(e),
    };
    let theirs = fetch_upstream(&repo, &upstream, credentials, app_private_key, None)?;
    let local = repo
        .head()
        .map_err(op_err)?
        .peel_to_commit()
        .map_err(op_err)?;
    let (ahead, behind) = repo
        .graph_ahead_behind(local.id(), theirs.id())
        .map_err(op_err)?;
    Ok(GitSyncStatus { ahead, behind })
}

pub fn reset_to_remote(
    repo_root: &Path,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let upstream = upstream_of_head(&repo)?;
    let theirs = fetch_upstream(&repo, &upstream, credentials, app_private_key, None)?;
    let commit = repo
        .find_object(theirs.id(), Some(git2::ObjectType::Commit))
        .map_err(op_err)?;
    repo.reset(
        &commit,
        ResetType::Hard,
        Some(CheckoutBuilder::default().force()),
    )
    .map_err(op_err)?;
    Ok(())
}

/// Current HEAD commit oid (hex) — used by the frontend action log to
/// capture a "before" snapshot ahead of a destructive operation (currently
/// just reset-to-remote) so it can be undone via `reset_to_oid`.
pub fn head_oid(repo_root: &Path) -> Result<String, GitError> {
    let repo = open_repo(repo_root)?;
    let commit = repo
        .head()
        .map_err(op_err)?
        .peel_to_commit()
        .map_err(op_err)?;
    Ok(commit.id().to_string())
}

/// Undoes `reset_to_remote()` (or any other hard-reset) by hard-resetting
/// back to a previously captured oid. Mirrors the hard-reset pattern
/// already used by `reset_to_remote`/`discard_tracked_changes`/`abort_merge`.
pub fn reset_to_oid(repo_root: &Path, oid: &str) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let oid = git2::Oid::from_str(oid)
        .map_err(|_| GitError::Message(format!("invalid commit id: {oid}")))?;
    let commit = repo
        .find_object(oid, Some(git2::ObjectType::Commit))
        .map_err(op_err)?;
    repo.reset(
        &commit,
        ResetType::Hard,
        Some(CheckoutBuilder::default().force()),
    )
    .map_err(op_err)?;
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
    let statuses = repo.statuses(Some(&mut opts)).map_err(op_err)?;
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

/// Line authorship for `path`, compacted into contiguous hunks that share
/// the same final commit. Optional `start_line`/`end_line` are 1-indexed
/// inclusive and passed straight to libgit2's blame options (`None` =
/// whole file). Returns an empty vec for an empty file.
pub fn blame(
    repo_root: &Path,
    path: &str,
    start_line: Option<u32>,
    end_line: Option<u32>,
) -> Result<Vec<GitBlameHunk>, GitError> {
    let rel = validate_relative_path(path)?;
    let repo = open_repo(repo_root)?;

    let mut opts = BlameOptions::new();
    if let Some(start) = start_line {
        opts.min_line(start.max(1) as usize);
    }
    if let Some(end) = end_line {
        opts.max_line(end.max(1) as usize);
    }

    let blame = repo
        .blame_file(rel, Some(&mut opts))
        .map_err(op_err)?;

    let mut hunks = Vec::new();
    for hunk in blame.iter() {
        let start = hunk.final_start_line() as u32;
        let lines = hunk.lines_in_hunk() as u32;
        if lines == 0 {
            continue;
        }
        let end = start.saturating_add(lines).saturating_sub(1);
        let oid = hunk.final_commit_id();
        let commit = short_oid(oid);
        let (author, authored_at) = match hunk.final_signature() {
            Some(sig) => (sig.name().unwrap_or("unknown").to_string(), format_git_time(sig.when())),
            None => ("unknown".into(), String::new()),
        };
        let summary = hunk
            .summary()
            .ok()
            .flatten()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        hunks.push(GitBlameHunk {
            start_line: start,
            end_line: end,
            commit,
            author,
            authored_at,
            summary,
        });
    }
    Ok(hunks)
}

/// Format a libgit2 signature timestamp as UTC ISO-8601 (`YYYY-MM-DDTHH:MM:SSZ`)
/// without pulling in chrono/time — epoch seconds are already UTC; the
/// signature's recorded offset is ignored so the AI tool payload stays
/// timezone-stable across machines.
fn format_git_time(time: git2::Time) -> String {
    let secs = time.seconds().max(0) as u64;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let min = (rem % 3_600) / 60;
    let sec = rem % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Convert days since Unix epoch to a Gregorian `(year, month, day)` —
/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

/// Write `content` as the new state for `path` at the given diff `scope`,
/// enabling partial (hunk-level) revert from the diff view — mirrors
/// IDEA's per-chunk "revert" arrows, which edit the modified pane and save
/// it back rather than discarding the whole file. For `Unstaged`, `content`
/// is written straight to the working tree file (the diff's "modified" side
/// is the workdir). For `Staged`, `content` is written as a new blob into
/// the index at that path, without touching the working tree (the diff's
/// "modified" side is the index) — so it doesn't disturb any separate
/// unstaged edits already sitting in the workdir for the same file.
pub fn apply_diff_content(
    repo_root: &Path,
    path: &str,
    scope: GitDiffScope,
    content: &str,
) -> Result<(), GitError> {
    let rel = validate_relative_path(path)?;
    let repo = open_repo(repo_root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Message("bare repository is not supported".into()))?;

    match scope {
        GitDiffScope::Unstaged => {
            let full = workdir.join(rel);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| GitError::Message(format!("failed to create directory: {e}")))?;
            }
            std::fs::write(&full, content)
                .map_err(|e| GitError::Message(format!("failed to write file: {e}")))?;
        }
        GitDiffScope::Staged => {
            let blob_id = repo.blob(content.as_bytes()).map_err(op_err)?;
            let mut index = repo.index().map_err(op_err)?;
            let mode = index.get_path(rel, 0).map(|e| e.mode).unwrap_or(0o100644);
            let entry = IndexEntry {
                ctime: IndexTime::new(0, 0),
                mtime: IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode,
                uid: 0,
                gid: 0,
                file_size: content.len() as u32,
                id: blob_id,
                flags: 0,
                flags_extended: 0,
                path: path.as_bytes().to_vec(),
            };
            index.add(&entry).map_err(op_err)?;
            index.write().map_err(op_err)?;
        }
    }
    Ok(())
}

/// Discards `path`'s uncommitted changes (staged + unstaged, or deletes an
/// untracked file/dir). Unlike the old implementation, this takes a backup
/// first via a pathspec-scoped stash, so the discard is undoable — see
/// `restore_discard_backup`. Returns `Some(backup_id)` (the stash oid) if
/// there was anything to discard, `None` if the path had no changes at all
/// (matches the previous no-op behavior — nothing to undo either).
///
/// Exception: on a brand-new repo with no commits yet (`repo.head()` fails
/// with `UnbornBranch`), libgit2's stash has nothing to diff against, so
/// this falls back to the old no-backup behavior for that narrow edge case
/// — discard remains non-undoable only when there is no history at all.
pub fn discard_file_changes(repo_root: &Path, path: &str) -> Result<Option<String>, GitError> {
    let rel = validate_relative_path(path)?;
    let mut repo = open_repo(repo_root)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Message("bare repository is not supported".into()))?
        .to_path_buf();

    let mut check_opts = StatusOptions::new();
    check_opts
        .pathspec(path)
        .include_untracked(true)
        .recurse_untracked_dirs(true);
    let has_changes = !repo
        .statuses(Some(&mut check_opts))
        .map_err(op_err)?
        .is_empty();
    if !has_changes {
        return Ok(None);
    }

    // Untracked file: back up its raw content as a git blob and delete it.
    // (Empirically, restoring an untracked-only pathspec-scoped stash via
    // `stash_apply` fails with libgit2's "1 conflict prevents checkout" —
    // a checkout-safety rejection, not a real content conflict, and not
    // worth chasing through libgit2's checkout-action internals when a
    // blob is simpler and more direct anyway: there's no tracked content
    // to 3-way-merge against, just bytes to put back.) Whole untracked
    // directories are a documented exception: a single blob can't capture
    // a directory's contents, so they're deleted with no backup, same as
    // the pre-existing behavior.
    if repo.head().is_err() {
        return discard_on_unborn_branch(&repo, &workdir, rel, path);
    }
    if path_is_untracked(&repo, path)? {
        let full = workdir.join(rel);
        if full.is_dir() {
            std::fs::remove_dir_all(&full)
                .map_err(|e| GitError::Message(format!("failed to remove directory: {e}")))?;
            return Ok(None);
        }
        if !full.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(&full)
            .map_err(|e| GitError::Message(format!("failed to read file: {e}")))?;
        let blob_oid = repo.blob(&bytes).map_err(op_err)?;
        std::fs::remove_file(&full)
            .map_err(|e| GitError::Message(format!("failed to remove file: {e}")))?;
        return Ok(Some(format!("{UNTRACKED_BACKUP_PREFIX}{blob_oid}:{path}")));
    }

    let sig = commit_signature(&repo)?;
    let mut save_opts = StashSaveOptions::new(sig);
    save_opts.pathspec(rel);
    let oid = repo
        .stash_save_ext(Some(&mut save_opts))
        .map_err(op_err)?;
    Ok(Some(oid.to_string()))
}

/// No commit exists yet (brand-new repo before the first commit) — neither
/// the stash backup nor the blob backup used above make sense to keep
/// consistent with libgit2's own requirements (stash needs a HEAD commit
/// to diff against), so this narrow edge case falls back to the original
/// no-backup behavior: discard remains non-undoable only when there is no
/// history at all yet.
fn discard_on_unborn_branch(
    repo: &Repository,
    workdir: &Path,
    rel: &Path,
    path: &str,
) -> Result<Option<String>, GitError> {
    if path_is_untracked(repo, path)? {
        let full = workdir.join(rel);
        if full.is_dir() {
            std::fs::remove_dir_all(&full)
                .map_err(|e| GitError::Message(format!("failed to remove directory: {e}")))?;
        } else if full.is_file() {
            std::fs::remove_file(&full)
                .map_err(|e| GitError::Message(format!("failed to remove file: {e}")))?;
        }
    } else {
        let mut index = repo.index().map_err(op_err)?;
        let _ = index.remove_path(rel);
        index.write().map_err(op_err)?;
        let full = workdir.join(rel);
        if full.is_dir() {
            std::fs::remove_dir_all(&full)
                .map_err(|e| GitError::Message(format!("failed to remove directory: {e}")))?;
        } else if full.is_file() {
            std::fs::remove_file(&full)
                .map_err(|e| GitError::Message(format!("failed to remove file: {e}")))?;
        }
    }
    Ok(None)
}

/// Prefix tagging a discard-backup id as a raw blob capture (untracked
/// file) rather than a stash oid (tracked file) — see `discard_file_changes`.
/// Format: `blob:<oid>:<repo-relative path>`. The oid is a fixed-length hex
/// SHA and never contains `:`, so splitting on the first `:` correctly
/// recovers the path even if the path itself contains one.
const UNTRACKED_BACKUP_PREFIX: &str = "blob:";

/// Positional stash index lookup by oid alone, no message-tag check —
/// unlike `find_stash_index_by_id` (which only matches `docflow-auto: `
/// branch-shelf entries), discard-backup stashes carry libgit2's default
/// "WIP on <branch>: ..." message (StashSaveOptions has no message setter
/// in this git2 version), so the caller already has the exact oid in hand
/// from `discard_file_changes`'s return value and doesn't need tag matching.
fn find_stash_index_by_oid_only(repo: &mut Repository, target: git2::Oid) -> Result<usize, GitError> {
    let mut found = None;
    repo.stash_foreach(|index, _message, oid| {
        if *oid == target {
            found = Some(index);
            return false;
        }
        true
    })
    .map_err(op_err)?;
    found.ok_or_else(|| GitError::StashNotFound(target.to_string()))
}

/// Restores a discard-backup — the "Undo" action for a discardFileChanges
/// log entry. Two forms, see `discard_file_changes`/`UNTRACKED_BACKUP_PREFIX`:
/// a `blob:<oid>:<path>` id writes the backed-up bytes straight back to
/// disk (untracked-file case); anything else is treated as a stash oid
/// (tracked-file case), applied via `stash_apply` — never `stash_pop`,
/// matching `do_stash_apply`'s conflict-safety discipline. A stash conflict
/// means the file was touched again since the discard; that's surfaced as
/// a plain error and the backup is left in place rather than inventing a
/// dedicated conflict-recovery UI for this single-path case.
pub fn restore_discard_backup(repo_root: &Path, backup_id: &str) -> Result<(), GitError> {
    if let Some(rest) = backup_id.strip_prefix(UNTRACKED_BACKUP_PREFIX) {
        let (oid_str, path) = rest
            .split_once(':')
            .ok_or_else(|| GitError::Message("некорректный идентификатор резервной копии".into()))?;
        let repo = open_repo(repo_root)?;
        let oid = git2::Oid::from_str(oid_str)
            .map_err(|_| GitError::StashNotFound(backup_id.to_string()))?;
        let blob = repo.find_blob(oid).map_err(op_err)?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| GitError::Message("bare repository is not supported".into()))?;
        let rel = validate_relative_path(path)?;
        let full = workdir.join(rel);
        if full.exists() {
            return Err(GitError::Message(
                "не удалось восстановить — по этому пути уже есть файл".into(),
            ));
        }
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| GitError::Message(format!("failed to create directory: {e}")))?;
        }
        std::fs::write(&full, blob.content())
            .map_err(|e| GitError::Message(format!("failed to write file: {e}")))?;
        return Ok(());
    }

    let mut repo = open_repo(repo_root)?;
    let target = git2::Oid::from_str(backup_id)
        .map_err(|_| GitError::StashNotFound(backup_id.to_string()))?;
    let index = find_stash_index_by_oid_only(&mut repo, target)?;
    repo.stash_apply(index, Some(&mut StashApplyOptions::new()))
        .map_err(op_err)?;
    if repo.index().map_err(op_err)?.has_conflicts() {
        return Err(GitError::Message(
            "не удалось восстановить — файл был изменён после отмены".into(),
        ));
    }
    let drop_index = find_stash_index_by_oid_only(&mut repo, target)?;
    repo.stash_drop(drop_index).map_err(op_err)?;
    Ok(())
}

/// Pick the remote to push a newly-tracked branch to: `origin` if present,
/// the sole remote if there's exactly one, otherwise ambiguous. Also reused
/// by `infra::repository_identity::resolve` to pick which remote's URL
/// identifies the repository — same "which remote is *the* remote"
/// question, so the same preference order applies.
pub(crate) fn default_remote_name(repo: &Repository) -> Result<String, GitError> {
    let remotes = repo.remotes().map_err(op_err)?;
    let names: Vec<&str> = remotes.iter().filter_map(|r| r.ok().flatten()).collect();
    if names.contains(&"origin") {
        return Ok("origin".to_string());
    }
    match names.as_slice() {
        [] => Err(GitError::Message(
            "repository has no configured remote".into(),
        )),
        [only] => Ok((*only).to_string()),
        _ => Err(GitError::Message(
            "multiple remotes configured; set an upstream branch manually before pushing".into(),
        )),
    }
}

fn push_refspec(
    repo: &Repository,
    remote_name: &str,
    local_branch: &str,
    remote_branch: &str,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
    on_progress: Option<&mut dyn FnMut(GitProgressEvent)>,
) -> Result<(), GitError> {
    let config = repo.config().map_err(op_err)?;
    let control = NetOpControl::new("push", on_progress, None);
    let mut callbacks = RemoteCallbacks::new();
    configure_credentials(&mut callbacks, &config, credentials, app_private_key, &control);
    configure_ssh_transport(&mut callbacks, credentials.trust_all_ssh_host_keys, &control);
    configure_sideband(&mut callbacks, &control);
    configure_push_progress(&mut callbacks, &control);

    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(callbacks);

    let mut remote = repo.find_remote(remote_name).map_err(map_remote_error)?;
    let refspec = format!("refs/heads/{local_branch}:refs/heads/{remote_branch}");
    remote
        .push(&[refspec.as_str()], Some(&mut push_opts))
        .map_err(map_remote_error)
}

pub fn push(
    repo_root: &Path,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
    on_progress: Option<&mut dyn FnMut(GitProgressEvent)>,
) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let head = repo.head().map_err(op_err)?;
    if !head.is_branch() {
        return Err(GitError::Message(
            "detached HEAD: check out a branch before pushing".into(),
        ));
    }
    let local_branch = head
        .shorthand()
        .map_err(|_| GitError::Message("cannot determine current branch".into()))?
        .to_string();

    match upstream_of_head(&repo) {
        Ok(upstream) => push_refspec(
            &repo,
            &upstream.remote_name,
            &local_branch,
            &upstream.remote_branch,
            credentials,
            app_private_key,
            on_progress,
        ),
        // No upstream yet — push to a sensible default remote and start tracking it,
        // mirroring `git push -u origin <branch>`.
        Err(GitError::NoUpstream) => {
            let remote_name = default_remote_name(&repo)?;
            push_refspec(
                &repo,
                &remote_name,
                &local_branch,
                &local_branch,
                credentials,
                app_private_key,
                on_progress,
            )?;
            let mut branch = repo
                .find_branch(&local_branch, BranchType::Local)
                .map_err(op_err)?;
            branch
                .set_upstream(Some(&format!("{remote_name}/{local_branch}")))
                .map_err(op_err)
        }
        Err(e) => Err(e),
    }
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
    let statuses = repo.statuses(Some(&mut opts)).map_err(op_err)?;
    Ok(statuses.iter().any(|e| {
        index_status_letter(e.status()).is_some()
            || tracked_workdir_status_letter(e.status()).is_some()
    }))
}

fn discard_tracked_changes(repo: &Repository) -> Result<(), GitError> {
    let head = repo.head().map_err(op_err)?;
    let commit = head.peel_to_commit().map_err(op_err)?;
    repo.reset(
        commit.as_object(),
        ResetType::Hard,
        Some(CheckoutBuilder::new().force()),
    )
    .map_err(op_err)?;
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

/// Message prefix tagging stash entries this app creates automatically when
/// switching branches with uncommitted tracked changes. Hand-made `git
/// stash` entries created outside the app (which won't have this prefix)
/// are filtered out everywhere below and never touched.
const STASH_TAG_PREFIX: &str = "docflow-auto: ";

fn stash_message_for_branch(branch: &str) -> String {
    format!("{STASH_TAG_PREFIX}{branch}")
}

/// libgit2 doesn't store the message we pass to `stash_save2` verbatim — it
/// wraps it as `"On <branch>: <our message>\n"` (see `stash.c`'s
/// `prepare_worktree_commit_message`, which splices in the branch name
/// itself before the colon). So the tag is searched for as a substring
/// rather than matched as a strict prefix.
fn parse_stash_branch(message: &str) -> Option<&str> {
    let idx = message.find(STASH_TAG_PREFIX)?;
    let rest = &message[idx + STASH_TAG_PREFIX.len()..];
    Some(rest.trim_end())
}

fn stash_entry_metadata(
    repo: &Repository,
    branch: String,
    oid: git2::Oid,
) -> Result<GitStashEntry, GitError> {
    let commit = repo.find_commit(oid).map_err(op_err)?;
    let files_changed = commit
        .parent(0)
        .ok()
        .and_then(|parent| parent.tree().ok())
        .and_then(|parent_tree| commit.tree().ok().map(|tree| (parent_tree, tree)))
        .and_then(|(parent_tree, tree)| {
            repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)
                .ok()
        })
        .map(|diff| diff.deltas().len())
        .unwrap_or(0);
    Ok(GitStashEntry {
        id: oid.to_string(),
        branch,
        created_at: commit.time().seconds(),
        files_changed,
    })
}

/// Stash all tracked, uncommitted changes (staged + unstaged) under a
/// `docflow-auto: <branch>` message tying the entry back to `branch`.
/// Untracked files are deliberately left alone, matching the pre-existing
/// checkout-blocking behavior. No-op if nothing tracked is dirty.
fn auto_stash_tracked_changes(
    repo: &mut Repository,
    branch: &str,
) -> Result<Option<GitStashEntry>, GitError> {
    if !has_tracked_uncommitted_changes(repo)? {
        return Ok(None);
    }
    let sig = commit_signature(repo)?;
    let oid = repo
        .stash_save2(&sig, Some(&stash_message_for_branch(branch)), None)
        .map_err(op_err)?;
    Ok(Some(stash_entry_metadata(repo, branch.to_string(), oid)?))
}

/// Re-resolves a shelf entry's *current* positional stash index from its
/// stable oid. Must be called fresh, immediately before `stash_apply`/
/// `stash_drop` — the index shifts every time any stash is pushed or
/// dropped, so it is never cached across calls.
fn find_stash_index_by_id(repo: &mut Repository, stash_id: &str) -> Result<usize, GitError> {
    let target = git2::Oid::from_str(stash_id)
        .map_err(|_| GitError::StashNotFound(stash_id.to_string()))?;
    let mut found = None;
    repo.stash_foreach(|index, message, oid| {
        if *oid == target && parse_stash_branch(message).is_some() {
            found = Some(index);
            return false;
        }
        true
    })
    .map_err(op_err)?;
    found.ok_or_else(|| GitError::StashNotFound(stash_id.to_string()))
}

fn find_stash_branch_for_oid(
    repo: &mut Repository,
    target: git2::Oid,
) -> Result<String, GitError> {
    let mut found = None;
    repo.stash_foreach(|_index, message, oid| {
        if *oid == target {
            found = parse_stash_branch(message).map(|b| b.to_string());
            return false;
        }
        true
    })
    .map_err(op_err)?;
    found.ok_or_else(|| GitError::StashNotFound(target.to_string()))
}

/// Applies one shelf entry by its stable stash-commit oid.
///
/// CRITICAL: uses `stash_apply`, never `stash_pop`. libgit2's `stash_pop` is
/// `stash_apply` followed by an *unconditional* `stash_drop` on success —
/// and `stash_apply` itself returns `Ok` even when the restore produced
/// conflicts (the final checkout runs without `ALLOW_CONFLICTS`, writing
/// conflict markers into the working tree and leaving the index conflicted,
/// exactly like this codebase's own `do_merge` already does for ordinary
/// merges). Using `stash_pop` here would silently drop the entry the moment
/// a restore conflicts — the exact data-loss trap this feature exists to
/// avoid. So conflicts are detected explicitly via `index.has_conflicts()`
/// after `stash_apply`, and `stash_drop` is only ever called from this
/// function, only on the fully-clean path.
fn do_stash_apply(
    repo: &mut Repository,
    stash_id: &str,
    entry: GitStashEntry,
) -> Result<GitStashRestoreOutcome, GitError> {
    let index = find_stash_index_by_id(repo, stash_id)?;
    match repo.stash_apply(index, Some(&mut StashApplyOptions::new())) {
        Ok(()) => {
            let has_conflicts = repo.index().map_err(op_err)?.has_conflicts();
            if has_conflicts {
                Ok(GitStashRestoreOutcome::Conflict { entry })
            } else {
                let drop_index = find_stash_index_by_id(repo, stash_id)?;
                repo.stash_drop(drop_index).map_err(op_err)?;
                Ok(GitStashRestoreOutcome::Applied { entry })
            }
        }
        Err(e) if e.code() == git2::ErrorCode::Uncommitted => Ok(GitStashRestoreOutcome::Blocked {
            entry,
            reason: "на этой ветке уже есть добавленные в индекс изменения".into(),
        }),
        Err(e) => Err(op_err(e)),
    }
}

/// Looks for docflow-managed shelf entries tagged for `branch`. Auto-restore
/// only happens when exactly one exists (unambiguous); with zero it's a
/// no-op, with two or more it's left for a manual pick in the shelf list
/// rather than guessing which one to apply.
fn maybe_auto_restore(
    repo: &mut Repository,
    branch: &str,
) -> Result<Option<GitStashRestoreOutcome>, GitError> {
    let mut raw = Vec::new();
    repo.stash_foreach(|_index, message, oid| {
        if parse_stash_branch(message) == Some(branch) {
            raw.push(*oid);
        }
        true
    })
    .map_err(op_err)?;

    match raw.len() {
        0 => Ok(None),
        1 => {
            let oid = raw[0];
            let entry = stash_entry_metadata(repo, branch.to_string(), oid)?;
            Ok(Some(do_stash_apply(repo, &oid.to_string(), entry)?))
        }
        n => Ok(Some(GitStashRestoreOutcome::Skipped { count: n })),
    }
}

/// List every docflow-managed shelf entry, newest first.
pub fn list_stash_shelf(repo_root: &Path) -> Result<Vec<GitStashEntry>, GitError> {
    let mut repo = open_repo(repo_root)?;
    let mut raw: Vec<(String, git2::Oid)> = Vec::new();
    repo.stash_foreach(|_index, message, oid| {
        if let Some(branch) = parse_stash_branch(message) {
            raw.push((branch.to_string(), *oid));
        }
        true
    })
    .map_err(op_err)?;

    let mut entries = raw
        .into_iter()
        .map(|(branch, oid)| stash_entry_metadata(&repo, branch, oid))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| std::cmp::Reverse(e.created_at));
    Ok(entries)
}

/// Manually apply a shelf entry (the "Восстановить" action in the shelf
/// UI). Requires the caller to already be on the branch the entry was
/// captured from — applying branch X's shelved edits onto branch Y's tree
/// is refused rather than guessed at.
pub fn apply_stash_entry(repo_root: &Path, stash_id: &str) -> Result<GitStashRestoreOutcome, GitError> {
    let mut repo = open_repo(repo_root)?;
    let oid = git2::Oid::from_str(stash_id)
        .map_err(|_| GitError::StashNotFound(stash_id.to_string()))?;
    let branch = find_stash_branch_for_oid(&mut repo, oid)?;
    let entry = stash_entry_metadata(&repo, branch.clone(), oid)?;

    if branch_name(&repo).as_deref() != Some(branch.as_str()) {
        return Err(GitError::Message(format!(
            "эти изменения относятся к ветке «{branch}» — переключитесь на неё, чтобы восстановить"
        )));
    }
    do_stash_apply(&mut repo, stash_id, entry)
}

/// Permanently delete a shelf entry (the "Удалить" action in the shelf UI —
/// the one genuinely destructive operation in the whole feature).
pub fn drop_stash_entry(repo_root: &Path, stash_id: &str) -> Result<(), GitError> {
    let mut repo = open_repo(repo_root)?;
    let index = find_stash_index_by_id(&mut repo, stash_id)?;
    repo.stash_drop(index).map_err(op_err)
}

fn switch_to_branch(repo: &Repository, branch_name: &str) -> Result<(), GitError> {
    let branch = repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|_| GitError::BranchNotFound(branch_name.to_string()))?;
    let commit = branch
        .get()
        .peel_to_commit()
        .map_err(op_err)?;
    let tree = commit.tree().map_err(op_err)?;
    repo.checkout_tree(tree.as_object(), Some(&mut CheckoutBuilder::new().force()))
        .map_err(op_err)?;
    repo.set_head(&format!("refs/heads/{branch_name}"))
        .map_err(op_err)?;
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
        .map_err(op_err)?;
    for branch_result in locals {
        let (branch, _) = branch_result.map_err(op_err)?;
        let name = branch
            .name()
            .map_err(op_err)?
            .ok_or_else(|| GitError::Message("branch has invalid name".into()))?
            .to_string();
        let behind = branch_behind_count(&repo, &branch);
        let tip_oid = branch.get().target().map(|oid| oid.to_string());
        out.push(GitBranchInfo {
            is_current: current.as_deref() == Some(name.as_str()),
            is_remote: false,
            behind,
            tip_oid,
            name,
        });
    }

    let remotes = repo
        .branches(Some(BranchType::Remote))
        .map_err(op_err)?;
    for branch_result in remotes {
        let (branch, _) = branch_result.map_err(op_err)?;
        let Some(name) = branch.name().map_err(op_err)? else {
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
            tip_oid: branch.get().target().map(|oid| oid.to_string()),
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
    on_progress: Option<&mut dyn FnMut(GitProgressEvent)>,
) -> Result<(), GitError> {
    let repo = open_repo(repo_root)?;
    let config = repo.config().map_err(op_err)?;
    let remote_names = repo.remotes().map_err(op_err)?;

    let control = NetOpControl::new("fetch", on_progress, None);

    for name in remote_names.iter() {
        let Ok(Some(name)) = name else { continue };
        control.set_op(format!("fetch:{name}"));
        let mut callbacks = RemoteCallbacks::new();
        configure_credentials(&mut callbacks, &config, credentials, app_private_key, &control);
        configure_ssh_transport(&mut callbacks, credentials.trust_all_ssh_host_keys, &control);
        configure_sideband(&mut callbacks, &control);
        configure_transfer_progress(&mut callbacks, &control);

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
    if discard_changes {
        ensure_clean_or_discard(&repo, true)?;
    }
    let head = repo.head().map_err(op_err)?;
    let commit = head.peel_to_commit().map_err(op_err)?;
    repo.branch(name, &commit, false)
        .map_err(op_err)?;
    // Same commit as HEAD — repoint HEAD only; a tree checkout would wipe staged
    // but uncommitted changes the user is carrying onto the new branch.
    repo.set_head(&format!("refs/heads/{name}"))
        .map_err(op_err)?;
    Ok(())
}

pub fn delete_branch(repo_root: &Path, name: &str) -> Result<(), GitError> {
    let name = validate_branch_name(name)?;
    let repo = open_repo(repo_root)?;
    if branch_name(&repo).as_deref() == Some(name) {
        return Err(GitError::CannotDeleteCurrentBranch);
    }
    let mut branch = repo
        .find_branch(name, BranchType::Local)
        .map_err(|_| GitError::BranchNotFound(name.to_string()))?;
    branch.delete().map_err(op_err)
}

/// Undoes `delete_branch()` by recreating a local branch at an explicit
/// commit oid captured by the caller before the delete — unlike
/// `create_branch`, which only branches from current HEAD. Doesn't check
/// out the new branch (delete_branch never switches either) and doesn't
/// restore upstream tracking (libgit2 discards that on delete, along with
/// the branch itself — an accepted, documented limitation of this undo).
pub fn create_branch_at_oid(repo_root: &Path, name: &str, oid: &str) -> Result<(), GitError> {
    let name = validate_branch_name(name)?;
    let repo = open_repo(repo_root)?;
    if repo.find_branch(name, BranchType::Local).is_ok() {
        return Err(GitError::BranchAlreadyExists(name.to_string()));
    }
    let oid = git2::Oid::from_str(oid)
        .map_err(|_| GitError::Message(format!("invalid commit id: {oid}")))?;
    let commit = repo.find_commit(oid).map_err(op_err)?;
    repo.branch(name, &commit, false)
        .map_err(op_err)?;
    Ok(())
}

/// Checks out `name`. If there are uncommitted tracked changes, they are
/// auto-stashed (tagged to the source branch) instead of blocking the
/// switch, and any unambiguous shelf entry for the destination branch is
/// auto-restored — see `auto_stash_tracked_changes`/`maybe_auto_restore`
/// and `do_stash_apply`'s doc comment for why this never silently drops
/// changes, even on conflict. `discard_changes` remains a hard-discard
/// escape hatch at the API level but is no longer driven by the checkout
/// UI, which always prefers stashing.
pub fn checkout_branch(
    repo_root: &Path,
    name: &str,
    discard_changes: bool,
) -> Result<CheckoutOutcome, GitError> {
    let name = validate_branch_name(name)?;
    let mut repo = open_repo(repo_root)?;
    if repo.find_branch(name, BranchType::Local).is_err() {
        return Err(GitError::BranchNotFound(name.to_string()));
    }

    let current = branch_name(&repo);
    if current.as_deref() == Some(name) {
        return Ok(CheckoutOutcome {
            shelved: None,
            restore: None,
        });
    }

    let shelved = if discard_changes {
        discard_tracked_changes(&repo)?;
        None
    } else {
        let source = current.unwrap_or_else(|| "detached".to_string());
        auto_stash_tracked_changes(&mut repo, &source)?
    };

    switch_to_branch(&repo, name)?;
    let restore = maybe_auto_restore(&mut repo, name)?;
    Ok(CheckoutOutcome { shelved, restore })
}

/// Check out a remote-tracking branch (e.g. `origin/feature-x`). If a local
/// branch with the same short name (`feature-x`) doesn't exist yet, it is
/// created tracking the remote branch; otherwise the existing local branch
/// is checked out as-is (mirrors `git checkout <remote-shorthand>`).
/// Auto-stash/auto-restore behavior mirrors `checkout_branch`.
pub fn checkout_remote_branch(
    repo_root: &Path,
    remote_branch_name: &str,
    discard_changes: bool,
) -> Result<CheckoutOutcome, GitError> {
    let mut repo = open_repo(repo_root)?;
    // Scoped so the immutable borrow `remote_branch` holds on `repo` ends
    // before the mutable borrows needed for auto-stashing below.
    let remote_commit_oid = {
        let remote_branch = repo
            .find_branch(remote_branch_name, BranchType::Remote)
            .map_err(|_| GitError::BranchNotFound(remote_branch_name.to_string()))?;
        remote_branch
            .get()
            .peel_to_commit()
            .map_err(op_err)?
            .id()
    };

    let local_name = remote_branch_name
        .split_once('/')
        .map(|(_, rest)| rest)
        .unwrap_or(remote_branch_name);
    let local_name = validate_branch_name(local_name)?.to_string();

    let current = branch_name(&repo);
    if current.as_deref() == Some(local_name.as_str()) {
        return Ok(CheckoutOutcome {
            shelved: None,
            restore: None,
        });
    }

    let shelved = if discard_changes {
        discard_tracked_changes(&repo)?;
        None
    } else {
        let source = current.unwrap_or_else(|| "detached".to_string());
        auto_stash_tracked_changes(&mut repo, &source)?
    };

    if repo.find_branch(&local_name, BranchType::Local).is_err() {
        let commit = repo
            .find_commit(remote_commit_oid)
            .map_err(op_err)?;
        let mut local_branch = repo
            .branch(&local_name, &commit, false)
            .map_err(op_err)?;
        local_branch
            .set_upstream(Some(remote_branch_name))
            .map_err(op_err)?;
    }

    switch_to_branch(&repo, &local_name)?;
    let restore = maybe_auto_restore(&mut repo, &local_name)?;
    Ok(CheckoutOutcome { shelved, restore })
}

/// Clone `url` into `destination`, reporting through `on_progress` and
/// stopping early whenever `is_cancelled` turns true.
///
/// `destination` may already exist as long as it is empty — that is the state
/// an aborted attempt leaves behind, and refusing it would lock the user out
/// of retrying into the same folder.
pub fn clone_repo<'a>(
    url: &str,
    destination: &Path,
    repo_config: &git2::Config,
    credentials: &GitCredentials,
    app_private_key: Option<&str>,
    // One lifetime for both: the progress sink is a `&mut` and therefore
    // invariant, so the cancel probe has to be borrowed for the same span.
    on_progress: Option<&'a mut dyn FnMut(GitProgressEvent)>,
    is_cancelled: Option<&'a (dyn Fn() -> bool + 'a)>,
) -> Result<Repository, GitError> {
    if directory_is_non_empty(destination) {
        return Err(GitError::DestinationExists(
            destination.display().to_string(),
        ));
    }

    let control = NetOpControl::new("clone", on_progress, is_cancelled);

    // Cancellation otherwise only takes effect at a libgit2 callback boundary;
    // this covers the case where the user gave up before anything started.
    if control.is_cancelled() {
        return Err(GitError::CloneFailed(CANCELLED_MESSAGE.into()));
    }

    let mut callbacks = RemoteCallbacks::new();
    configure_credentials(
        &mut callbacks,
        repo_config,
        credentials,
        app_private_key,
        &control,
    );
    configure_ssh_transport(&mut callbacks, credentials.trust_all_ssh_host_keys, &control);
    configure_sideband(&mut callbacks, &control);
    configure_transfer_progress(&mut callbacks, &control);

    let mut fetch_opts = FetchOptions::new(); // clone
    fetch_opts.remote_callbacks(callbacks);

    // Checkout writes the working tree, and it is where a clone that leaves
    // nothing but a `.git` directory behind is stuck. libgit2 offers no way to
    // abort from here — the callback returns `()` — so this reports only, and
    // cancellation during checkout is the caller's problem.
    let mut checkout = CheckoutBuilder::new();
    checkout.progress(|path, completed, total| {
        control.emit(GitProgressEvent::Checkout {
            op: control.op(),
            completed,
            total,
            path: path.map(|p| p.to_string_lossy().into_owned()),
        });
    });

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_opts);
    builder.with_checkout(checkout);

    control.phase(GitPhase::Connecting, Some(url.to_string()));

    builder
        .clone(url, destination)
        .map_err(|e| GitError::CloneFailed(e.message().to_string()))
}

/// True when `path` is a directory that already holds something. Used instead
/// of a bare `exists()` check so an empty leftover directory is not treated as
/// a conflict — `services::git_clone` applies the same rule up front, and the
/// UI mirrors it through `check_path_exists`.
pub fn directory_is_non_empty(path: &Path) -> bool {
    path.read_dir()
        .ok()
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
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
    fn clone_repo_destination_non_empty() {
        let dir = temp_dir("clone-exists");
        let dest = dir.join("repo");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("stale.txt"), "old\n").unwrap();

        let config = git2::Config::open_default().unwrap();
        let creds = GitCredentials::default();
        let result = clone_repo("https://example.com/repo.git", &dest, &config, &creds, None, None, None);
        assert!(
            matches!(result, Err(GitError::DestinationExists(_))),
            "expected DestinationExists, but got a different result"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_repo_accepts_existing_empty_destination() {
        let dir = temp_dir("clone-empty-dest");
        let dest = dir.join("repo");
        fs::create_dir_all(&dest).unwrap();

        let config = git2::Config::open_default().unwrap();
        let creds = GitCredentials::default();
        let result = clone_repo("not-a-valid-url", &dest, &config, &creds, None, None, None);
        // It still fails — on the URL, not on the directory. An empty leftover
        // folder from an aborted attempt must not block a retry.
        assert!(
            !matches!(result, Err(GitError::DestinationExists(_))),
            "an existing empty destination must not be rejected"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_repo_stops_when_already_cancelled() {
        let dir = temp_dir("clone-cancel");
        let remote = dir.join("remote.git");
        Repository::init_bare(&remote).unwrap();
        let dest = dir.join("dest");

        let config = git2::Config::open_default().unwrap();
        let creds = GitCredentials::default();
        // Cancellation mid-flight is observed at libgit2 callback boundaries;
        // a clone that is already cancelled must not start at all.
        let cancelled = || true;
        let result = clone_repo(
            remote.to_str().unwrap(),
            &dest,
            &config,
            &creds,
            None,
            None,
            Some(&cancelled),
        );
        assert!(result.is_err(), "a cancelled clone must not succeed");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clone_repo_invalid_url() {
        let dir = temp_dir("clone-invalid-url");
        let dest = dir.join("empty");

        let config = git2::Config::open_default().unwrap();
        let creds = GitCredentials::default();
        let result = clone_repo("not-a-valid-url", &dest, &config, &creds, None, None, None);
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
        let repo = clone_repo(remote.to_str().unwrap(), &dest, &config, &creds, None, None, None).unwrap();
        assert!(dest.join("README.md").exists());
        assert!(!repo.is_bare());

        fs::remove_dir_all(&base).ok();
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn push_sets_upstream_when_missing() {
        let base = temp_dir("push-no-upstream");
        let remote = base.join("remote");
        let local = base.join("local");

        Repository::init_bare(&remote).unwrap();

        let repo = Repository::init(&local).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        fs::write(local.join("README.md"), "# Hello\n").unwrap();
        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("README.md")).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            let sig = commit_signature(&repo).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }
        repo.remote("origin", remote.to_str().unwrap()).unwrap();

        // Precondition: freshly committed branch has no upstream yet.
        assert!(matches!(
            upstream_of_head(&repo),
            Err(GitError::NoUpstream)
        ));

        let creds = GitCredentials::default();
        push(&local, &creds, None, None).unwrap();

        let branch = repo.find_branch("main", BranchType::Local).ok().or_else(|| {
            let name = branch_name(&repo)?;
            repo.find_branch(&name, BranchType::Local).ok()
        });
        let branch = branch.expect("local branch should exist after push");
        let upstream = branch.upstream().expect("upstream should now be set");
        assert!(upstream
            .name()
            .unwrap()
            .unwrap()
            .starts_with("origin/"));

        // Bare remote should have received the commit.
        let remote_repo = Repository::open(&remote).unwrap();
        assert!(remote_repo.head().is_ok());

        fs::remove_dir_all(&base).ok();
    }

    fn commit_file(repo: &Repository, name: &str, content: &str, message: &str) {
        fs::write(repo.workdir().unwrap().join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = commit_signature(repo).unwrap();
        let parents: Vec<_> = repo.head().ok().and_then(|h| h.peel_to_commit().ok()).into_iter().collect();
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
            .unwrap();
    }

    fn delete_and_commit(repo: &Repository, name: &str, message: &str) {
        std::fs::remove_file(repo.workdir().unwrap().join(name)).unwrap();
        let mut index = repo.index().unwrap();
        index.remove_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = commit_signature(repo).unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
            .unwrap();
    }

    #[test]
    fn commit_files_lists_added_files_on_root_commit() {
        let dir = temp_dir("commit-files-root");
        let repo = Repository::init(&dir).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "init");
        let head = repo.head().unwrap().peel_to_commit().unwrap();

        let files = commit_files(&dir, &head.id().to_string()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "a.txt");
        assert_eq!(files[0].status, "A");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn commit_files_lists_modified_and_deleted_files() {
        let dir = temp_dir("commit-files-mod-del");
        let repo = Repository::init(&dir).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "init");
        commit_file(&repo, "b.txt", "two", "add b");
        commit_file(&repo, "a.txt", "one-changed", "modify a");
        delete_and_commit(&repo, "b.txt", "remove b");

        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let files = commit_files(&dir, &head.id().to_string()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "b.txt");
        assert_eq!(files[0].status, "D");

        let modify_commit = head.parent(0).unwrap();
        let files = commit_files(&dir, &modify_commit.id().to_string()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "a.txt");
        assert_eq!(files[0].status, "M");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn commit_file_diff_returns_before_and_after_content() {
        let dir = temp_dir("commit-file-diff");
        let repo = Repository::init(&dir).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "init");
        commit_file(&repo, "a.txt", "two", "modify a");
        let head = repo.head().unwrap().peel_to_commit().unwrap();

        let diff = commit_file_diff(&dir, &head.id().to_string(), "a.txt").unwrap();
        assert_eq!(diff.original, "one");
        assert_eq!(diff.modified, "two");
        assert!(!diff.is_binary);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn blame_compacts_contiguous_lines_from_the_same_commit() {
        let dir = temp_dir("blame-basic");
        let repo = Repository::init(&dir).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Alice").unwrap();
            config.set_str("user.email", "alice@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "line1\nline2\n", "init");
        commit_file(&repo, "a.txt", "line1\nline2\nline3\n", "append");

        let hunks = blame(&dir, "a.txt", None, None).unwrap();
        assert!(!hunks.is_empty());
        assert_eq!(hunks[0].start_line, 1);
        assert!(!hunks[0].commit.is_empty());
        assert_eq!(hunks[0].author, "Alice");
        assert!(hunks[0].authored_at.ends_with('Z'));
        assert!(hunks.iter().any(|h| h.summary.contains("init") || h.summary.contains("append")));

        let ranged = blame(&dir, "a.txt", Some(3), Some(3)).unwrap();
        assert_eq!(ranged.len(), 1);
        assert_eq!(ranged[0].start_line, 3);
        assert_eq!(ranged[0].end_line, 3);
        assert!(ranged[0].summary.contains("append"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn format_git_time_formats_unix_epoch_as_utc_iso() {
        let t = git2::Time::new(0, 0);
        assert_eq!(format_git_time(t), "1970-01-01T00:00:00Z");
        let t = git2::Time::new(1_000_000_000, 0);
        assert_eq!(format_git_time(t), "2001-09-09T01:46:40Z");
    }

    #[test]
    fn commit_file_diff_root_commit_has_empty_original() {
        let dir = temp_dir("commit-file-diff-root");
        let repo = Repository::init(&dir).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "init");
        let head = repo.head().unwrap().peel_to_commit().unwrap();

        let diff = commit_file_diff(&dir, &head.id().to_string(), "a.txt").unwrap();
        assert_eq!(diff.original, "");
        assert_eq!(diff.modified, "one");
        assert_eq!(diff.original_label, "(empty)");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unpushed_status_reports_no_commits_on_fresh_repo() {
        let dir = temp_dir("unpushed-empty");
        let repo = Repository::init(&dir).unwrap();

        let status = unpushed_status(&repo);
        assert!(!status.has_commits);
        assert!(!status.has_upstream);
        assert_eq!(status.ahead, 0);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unpushed_status_reports_has_commits_without_upstream() {
        let dir = temp_dir("unpushed-no-upstream");
        let repo = Repository::init(&dir).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "first");

        let status = unpushed_status(&repo);
        assert!(status.has_commits);
        assert!(!status.has_upstream);
        assert_eq!(status.ahead, 0);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unpushed_status_reports_ahead_count_with_upstream() {
        let base = temp_dir("unpushed-ahead");
        let remote = base.join("remote");
        let local = base.join("local");

        Repository::init_bare(&remote).unwrap();
        let repo = Repository::init(&local).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "first");
        repo.remote("origin", remote.to_str().unwrap()).unwrap();
        push(&local, &GitCredentials::default(), None, None).unwrap();

        // In sync right after the initial push.
        let status = unpushed_status(&repo);
        assert!(status.has_commits);
        assert!(status.has_upstream);
        assert_eq!(status.ahead, 0);

        // One more local commit, not yet pushed.
        commit_file(&repo, "b.txt", "two", "second");
        let status = unpushed_status(&repo);
        assert!(status.has_upstream);
        assert_eq!(status.ahead, 1);

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn unpushed_commits_lists_commits_without_upstream() {
        let dir = temp_dir("unpushed-commits-no-upstream");
        let repo = Repository::init(&dir).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "first");

        let commits = unpushed_commits(&dir, 50).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "first");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unpushed_commits_lists_only_ahead_commits_with_upstream() {
        let base = temp_dir("unpushed-commits-ahead");
        let remote = base.join("remote");
        let local = base.join("local");

        Repository::init_bare(&remote).unwrap();
        let repo = Repository::init(&local).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "first");
        repo.remote("origin", remote.to_str().unwrap()).unwrap();
        push(&local, &GitCredentials::default(), None, None).unwrap();

        assert!(unpushed_commits(&local, 50).unwrap().is_empty());

        commit_file(&repo, "b.txt", "two", "second");
        let commits = unpushed_commits(&local, 50).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "second");

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn incoming_commits_lists_remote_commits_not_on_local() {
        let base = temp_dir("incoming-commits");
        let remote = base.join("remote");
        let local = base.join("local");
        let other = base.join("other");

        Repository::init_bare(&remote).unwrap();

        let repo = Repository::init(&local).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "first");
        repo.remote("origin", remote.to_str().unwrap()).unwrap();
        push(&local, &GitCredentials::default(), None, None).unwrap();

        let other_repo = Repository::clone(remote.to_str().unwrap(), &other).unwrap();
        {
            let mut config = other_repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&other_repo, "b.txt", "two", "remote-only");
        push(&other, &GitCredentials::default(), None, None).unwrap();

        let commits = incoming_commits(
            &local,
            &GitCredentials::default(),
            None,
            50,
        )
        .unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "remote-only");

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn create_branch_preserves_staged_changes() {
        let dir = temp_dir("create-branch-dirty");
        let repo = Repository::init(&dir).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "init");
        fs::write(dir.join("b.txt"), "draft").unwrap();
        stage_paths(&dir, &["b.txt".to_string()]).unwrap();

        create_branch(&dir, "feature", false).unwrap();

        let snapshot = status(&dir).unwrap();
        assert_eq!(snapshot.branch.as_deref(), Some("feature"));
        assert_eq!(snapshot.staged.len(), 1);
        assert_eq!(snapshot.staged[0].path, "b.txt");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn move_unpushed_to_new_branch_leaves_source_at_upstream() {
        let base = temp_dir("move-unpushed-new");
        let remote = base.join("remote");
        let local = base.join("local");

        Repository::init_bare(&remote).unwrap();
        let repo = Repository::init(&local).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "first");
        repo.remote("origin", remote.to_str().unwrap()).unwrap();
        push(&local, &GitCredentials::default(), None, None).unwrap();
        commit_file(&repo, "b.txt", "two", "wrong-branch");

        move_unpushed_to_new_branch(&local, "feature").unwrap();

        let snapshot = status(&local).unwrap();
        assert_eq!(snapshot.branch.as_deref(), Some("feature"));
        assert_eq!(log(&local, 5).unwrap()[0].message, "wrong-branch");

        repo.set_head("refs/heads/master").unwrap();
        repo.checkout_head(Some(CheckoutBuilder::default().force()))
            .unwrap();
        let on_master = log(&local, 5).unwrap();
        assert_eq!(on_master.len(), 1);
        assert_eq!(on_master[0].message, "first");
        assert_eq!(status(&local).unwrap().ahead, 0);

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn drop_unpushed_from_removes_selected_and_newer() {
        let base = temp_dir("drop-unpushed-from");
        let remote = base.join("remote");
        let local = base.join("local");

        Repository::init_bare(&remote).unwrap();
        let repo = Repository::init(&local).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "one", "first");
        repo.remote("origin", remote.to_str().unwrap()).unwrap();
        push(&local, &GitCredentials::default(), None, None).unwrap();
        commit_file(&repo, "b.txt", "two", "second");
        commit_file(&repo, "c.txt", "three", "third");
        let third = log(&local, 10).unwrap()[0].hash.clone();

        drop_unpushed_from(&local, &third, GitResetMode::Hard).unwrap();

        let commits = unpushed_commits(&local, 10).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "second");

        fs::remove_dir_all(&base).ok();
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

    /// Sets up a repo with a diverging local branch, then merges `theirs`
    /// into HEAD so a genuine content conflict on `a.txt` is produced —
    /// mirrors what `pull(..., PullMode::Merge, ...)` does against a remote.
    fn setup_conflicting_merge(dir: &Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        commit_file(&repo, "a.txt", "base\n", "init");
        let base_branch = branch_name(&repo).unwrap();

        repo.branch(
            "theirs",
            &repo.head().unwrap().peel_to_commit().unwrap(),
            false,
        )
        .unwrap();
        commit_file(&repo, "a.txt", "ours\n", "ours change");

        repo.set_head("refs/heads/theirs").unwrap();
        repo.checkout_head(Some(CheckoutBuilder::default().force()))
            .unwrap();
        commit_file(&repo, "a.txt", "theirs\n", "theirs change");
        let theirs_id = repo.head().unwrap().peel_to_commit().unwrap().id();

        repo.set_head(&format!("refs/heads/{base_branch}")).unwrap();
        repo.checkout_head(Some(CheckoutBuilder::default().force()))
            .unwrap();

        {
            let theirs_ann = repo.find_annotated_commit(theirs_id).unwrap();
            let err = do_merge(&repo, &theirs_ann).unwrap_err();
            assert!(matches!(err, GitError::MergeConflict));
        }
        repo
    }

    #[test]
    fn conflicted_merge_leaves_merge_state_and_marker_file() {
        let dir = temp_dir("conflict-setup");
        let repo = setup_conflicting_merge(&dir);

        assert_eq!(repo.state(), RepositoryState::Merge);
        let index = repo.index().unwrap();
        assert!(index.has_conflicts());

        let on_disk = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(on_disk.contains("<<<<<<<"));
        assert!(on_disk.contains("======="));
        assert!(on_disk.contains(">>>>>>>"));

        let status = status(&dir).unwrap();
        assert_eq!(status.conflicted.len(), 1);
        assert_eq!(status.conflicted[0].path, "a.txt");
        assert_eq!(status.conflicted[0].status, "U");
        assert!(status.staged.is_empty());
        assert!(status.unstaged.is_empty());
        assert!(status.merge_in_progress);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_conflict_rejects_remaining_markers() {
        let dir = temp_dir("conflict-reject-markers");
        setup_conflicting_merge(&dir);

        let err = resolve_conflict(&dir, "a.txt", "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> theirs\n")
            .unwrap_err();
        assert!(matches!(err, GitError::Message(_)));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_conflict_then_finish_merge_creates_two_parent_commit() {
        let dir = temp_dir("conflict-finish");
        let repo = setup_conflicting_merge(&dir);
        let ours_head = repo.head().unwrap().peel_to_commit().unwrap();

        resolve_conflict(&dir, "a.txt", "resolved\n").unwrap();

        let after_resolve = status(&dir).unwrap();
        assert!(after_resolve.conflicted.is_empty());
        assert!(after_resolve.merge_in_progress);

        let short_hash = finish_merge(&dir).unwrap();
        assert_eq!(short_hash.len(), 7);

        assert_eq!(repo.state(), RepositoryState::Clean);
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.parent_count(), 2);
        assert_eq!(head.parent(0).unwrap().id(), ours_head.id());

        let content = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert_eq!(content, "resolved\n");

        let after_finish = status(&dir).unwrap();
        assert!(after_finish.conflicted.is_empty());
        assert!(!after_finish.merge_in_progress);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn abort_merge_restores_head_content_and_clears_state() {
        let dir = temp_dir("conflict-abort");
        let repo = setup_conflicting_merge(&dir);
        let ours_head = repo.head().unwrap().peel_to_commit().unwrap();

        abort_merge(&dir).unwrap();

        assert_eq!(repo.state(), RepositoryState::Clean);
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.id(), ours_head.id());

        let content = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert_eq!(content, "ours\n");

        let after_abort = status(&dir).unwrap();
        assert!(after_abort.conflicted.is_empty());
        assert!(!after_abort.merge_in_progress);

        fs::remove_dir_all(&dir).ok();
    }

    /// Reproduces the state a user can end up in outside a clean `do_merge`
    /// call — e.g. an interrupted merge — where the index still holds
    /// conflicted entries but `MERGE_HEAD` is gone, so `repo.state()` reports
    /// `Clean` instead of `Merge`. Before this fix, `abort_merge` refused to
    /// run in this case (guarded on `RepositoryState::Merge` alone), leaving
    /// the user with conflicted files and no way to clear them from the app.
    #[test]
    fn abort_merge_recovers_conflicted_index_without_merge_head() {
        let dir = temp_dir("conflict-abort-no-merge-head");
        let repo = setup_conflicting_merge(&dir);
        let ours_head = repo.head().unwrap().peel_to_commit().unwrap();

        // Simulate MERGE_HEAD having been lost while the index conflict
        // remains — `cleanup_state` clears MERGE_HEAD/MERGE_MSG but leaves
        // the index and working tree untouched.
        repo.cleanup_state().unwrap();
        assert_eq!(repo.state(), RepositoryState::Clean);

        let before_abort = status(&dir).unwrap();
        assert_eq!(before_abort.conflicted.len(), 1);
        assert!(!before_abort.merge_in_progress);

        abort_merge(&dir).unwrap();

        assert_eq!(repo.state(), RepositoryState::Clean);
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.id(), ours_head.id());

        let content = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert_eq!(content, "ours\n");

        let after_abort = status(&dir).unwrap();
        assert!(after_abort.conflicted.is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    /// End-to-end reproduction of what the app does when the user clicks
    /// Git → "Обновить проект" → merge: two clones of a bare remote diverge
    /// on the same line of the same file, one side is pushed, and `pull()`
    /// is called on the other exactly as `git_ops::pull` calls it.
    #[test]
    fn pull_merge_through_full_fetch_path_leaves_conflict_resolvable() {
        let base = temp_dir("pull-conflict-e2e");
        let remote = base.join("remote");
        let local_a = base.join("a");
        let local_b = base.join("b");

        Repository::init_bare(&remote).unwrap();

        let repo_a = Repository::init(&local_a).unwrap();
        {
            let mut config = repo_a.config().unwrap();
            config.set_str("user.name", "A").unwrap();
            config.set_str("user.email", "a@test.com").unwrap();
        }
        commit_file(&repo_a, "a.txt", "base\n", "init");
        repo_a.remote("origin", remote.to_str().unwrap()).unwrap();
        let creds = GitCredentials::default();
        push(&local_a, &creds, None, None).unwrap();

        let repo_b = Repository::clone(remote.to_str().unwrap(), &local_b).unwrap();
        {
            let mut config = repo_b.config().unwrap();
            config.set_str("user.name", "B").unwrap();
            config.set_str("user.email", "b@test.com").unwrap();
        }

        // A changes a.txt and pushes.
        commit_file(&repo_a, "a.txt", "from A\n", "A change");
        push(&local_a, &creds, None, None).unwrap();

        // B changes the same line locally without pulling first, then pulls.
        commit_file(&repo_b, "a.txt", "from B\n", "B change");

        let err = pull(&local_b, PullMode::Merge, &creds, None, None).unwrap_err();
        assert!(matches!(err, GitError::MergeConflict), "expected MergeConflict, got {err:?}");

        assert_eq!(repo_b.state(), RepositoryState::Merge);
        let status_b = status(&local_b).unwrap();
        assert_eq!(status_b.conflicted.len(), 1);
        assert_eq!(status_b.conflicted[0].path, "a.txt");
        assert!(status_b.merge_in_progress);

        let on_disk = std::fs::read_to_string(local_b.join("a.txt")).unwrap();
        assert!(on_disk.contains("<<<<<<<"));

        resolve_conflict(&local_b, "a.txt", "resolved\n").unwrap();
        let hash = finish_merge(&local_b).unwrap();
        assert_eq!(hash.len(), 7);

        let after = status(&local_b).unwrap();
        assert!(after.conflicted.is_empty());
        assert!(!after.merge_in_progress);

        fs::remove_dir_all(&base).ok();
    }

    fn init_repo_with_identity(dir: &Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@test.com").unwrap();
        repo
    }

    #[test]
    fn checkout_branch_auto_stashes_and_restores_cleanly_on_return() {
        let dir = temp_dir("stash-roundtrip");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "f.txt", "line1\n", "init");
        let base_name = branch_name(&repo).unwrap();

        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &head_commit, false).unwrap();

        // Uncommitted tracked edit on the source branch — this is what
        // should get shelved instead of blocking the switch.
        fs::write(dir.join("f.txt"), "line1\nlocal-edit\n").unwrap();

        let outcome = checkout_branch(&dir, "feature", false).unwrap();
        assert!(outcome.shelved.is_some(), "expected the edit to be auto-stashed");
        assert!(outcome.restore.is_none(), "no shelf entry existed for 'feature' yet");
        assert_eq!(branch_name(&repo).as_deref(), Some("feature"));
        assert_eq!(
            fs::read_to_string(dir.join("f.txt")).unwrap(),
            "line1\n",
            "feature's working tree should be clean, not carrying the stashed edit"
        );

        let shelf = list_stash_shelf(&dir).unwrap();
        assert_eq!(shelf.len(), 1);
        assert_eq!(shelf[0].branch, base_name);

        let outcome = checkout_branch(&dir, &base_name, false).unwrap();
        assert!(outcome.shelved.is_none(), "feature had no uncommitted changes to shelve");
        match outcome.restore {
            Some(GitStashRestoreOutcome::Applied { .. }) => {}
            other => panic!("expected a clean Applied restore, got {other:?}"),
        }
        assert_eq!(
            fs::read_to_string(dir.join("f.txt")).unwrap(),
            "line1\nlocal-edit\n",
            "the shelved edit should have been restored"
        );
        assert!(
            list_stash_shelf(&dir).unwrap().is_empty(),
            "the shelf entry should be dropped after a clean apply"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn checkout_branch_restore_conflict_keeps_shelf_entry_intact() {
        // Reproduces the scenario the auto-stash feature exists to protect
        // against: the destination branch advances (e.g. via a pull that
        // happened while the user was away on another branch) in a way that
        // overlaps the shelved edit. The restore must NOT silently drop the
        // stash — it has to stay in the shelf, visible and recoverable.
        let dir = temp_dir("stash-conflict");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "f.txt", "line1\n", "init");
        let base_name = branch_name(&repo).unwrap();
        let base_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &base_commit, false).unwrap();

        // Shelve an uncommitted edit on the base branch by switching away.
        fs::write(dir.join("f.txt"), "line1\nlocal-edit\n").unwrap();
        let outcome = checkout_branch(&dir, "feature", false).unwrap();
        assert!(outcome.shelved.is_some());
        assert_eq!(branch_name(&repo).as_deref(), Some("feature"));

        // Simulate the base branch advancing without us being checked out
        // on it (e.g. a background pull performed via another tool),
        // touching the exact same insertion point as the shelved edit.
        let new_blob = repo.blob(b"line1\nremote-edit\n").unwrap();
        let mut treebuilder = repo
            .treebuilder(Some(&base_commit.tree().unwrap()))
            .unwrap();
        treebuilder.insert("f.txt", new_blob, 0o100644).unwrap();
        let new_tree_oid = treebuilder.write().unwrap();
        let new_tree = repo.find_tree(new_tree_oid).unwrap();
        let sig = commit_signature(&repo).unwrap();
        let new_commit_oid = repo
            .commit(None, &sig, &sig, "simulated remote update", &new_tree, &[&base_commit])
            .unwrap();
        let mut base_branch = repo.find_branch(&base_name, BranchType::Local).unwrap();
        base_branch
            .get_mut()
            .set_target(new_commit_oid, "simulated remote update")
            .unwrap();

        // feature is clean, so switching back to base only exercises the
        // restore path, not another auto-stash.
        let outcome = checkout_branch(&dir, &base_name, false).unwrap();
        assert!(outcome.shelved.is_none());
        let entry = match outcome.restore {
            Some(GitStashRestoreOutcome::Conflict { entry }) => entry,
            other => panic!("expected a Conflict restore outcome, got {other:?}"),
        };
        assert_eq!(entry.branch, base_name);

        // The shelf entry must still be there — nothing was silently lost.
        let shelf = list_stash_shelf(&dir).unwrap();
        assert_eq!(shelf.len(), 1, "conflicted restore must not drop the stash entry");
        assert_eq!(shelf[0].id, entry.id);

        // And the conflict is visible through the normal status surface.
        let snapshot = status(&dir).unwrap();
        assert_eq!(snapshot.conflicted.len(), 1);
        assert_eq!(snapshot.conflicted[0].path, "f.txt");

        // Resolving the conflict and manually dropping the (now redundant)
        // shelf entry should work via the same public API the shelf UI uses.
        resolve_conflict(&dir, "f.txt", "line1\nresolved\n").unwrap();
        drop_stash_entry(&dir, &entry.id).unwrap();
        assert!(list_stash_shelf(&dir).unwrap().is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apply_stash_entry_refuses_when_not_on_the_shelved_branch() {
        let dir = temp_dir("stash-wrong-branch");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "f.txt", "line1\n", "init");
        let base_name = branch_name(&repo).unwrap();
        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &head_commit, false).unwrap();

        fs::write(dir.join("f.txt"), "line1\nlocal-edit\n").unwrap();
        checkout_branch(&dir, "feature", false).unwrap();

        let shelf = list_stash_shelf(&dir).unwrap();
        assert_eq!(shelf.len(), 1);
        assert_eq!(shelf[0].branch, base_name);

        // Still on 'feature' — applying the base branch's shelf entry
        // directly (the manual "Восстановить" action) must be refused
        // rather than silently applied onto the wrong branch's tree.
        let err = apply_stash_entry(&dir, &shelf[0].id).unwrap_err();
        assert!(matches!(err, GitError::Message(_)));
        assert_eq!(list_stash_shelf(&dir).unwrap().len(), 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn undo_commit_soft_resets_to_parent_and_keeps_index() {
        let dir = temp_dir("undo-commit");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "a.txt", "one", "init");

        fs::write(dir.join("a.txt"), "two").unwrap();
        stage_paths(&dir, &["a.txt".to_string()]).unwrap();
        let hash = commit(&dir, "second").unwrap();

        undo_commit(&dir, &hash).unwrap();

        let history = log(&dir, 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].message, "init");

        let snapshot = status(&dir).unwrap();
        assert_eq!(snapshot.staged.len(), 1);
        assert_eq!(snapshot.staged[0].path, "a.txt");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn undo_commit_refuses_when_head_has_moved_on() {
        let dir = temp_dir("undo-commit-stale");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "a.txt", "one", "init");
        let first_hash = commit_file_and_commit(&dir, "a.txt", "two", "second");
        commit_file(&repo, "a.txt", "three", "third");

        let err = undo_commit(&dir, &first_hash).unwrap_err();
        assert!(matches!(err, GitError::Message(_)));

        // History untouched — still three commits.
        assert_eq!(log(&dir, 10).unwrap().len(), 3);

        fs::remove_dir_all(&dir).ok();
    }

    fn commit_file_and_commit(dir: &Path, name: &str, content: &str, message: &str) -> String {
        fs::write(dir.join(name), content).unwrap();
        stage_paths(dir, &[name.to_string()]).unwrap();
        commit(dir, message).unwrap()
    }

    #[test]
    fn create_branch_at_oid_recreates_a_deleted_branch() {
        let dir = temp_dir("create-branch-at-oid");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "a.txt", "one", "init");
        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        let tip_oid = head_commit.id().to_string();

        repo.branch("feature", &head_commit, false).unwrap();
        delete_branch(&dir, "feature").unwrap();
        assert!(repo.find_branch("feature", BranchType::Local).is_err());

        create_branch_at_oid(&dir, "feature", &tip_oid).unwrap();
        let recreated = repo.find_branch("feature", BranchType::Local).unwrap();
        assert_eq!(recreated.get().target().unwrap().to_string(), tip_oid);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_branch_at_oid_refuses_when_branch_already_exists() {
        let dir = temp_dir("create-branch-at-oid-exists");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "a.txt", "one", "init");
        let oid = repo.head().unwrap().peel_to_commit().unwrap().id().to_string();

        let err = create_branch_at_oid(&dir, &branch_name(&repo).unwrap(), &oid).unwrap_err();
        assert!(matches!(err, GitError::BranchAlreadyExists(_)));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn head_oid_and_reset_to_oid_round_trip() {
        let dir = temp_dir("reset-to-oid");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "a.txt", "one", "init");
        let before = head_oid(&dir).unwrap();

        commit_file(&repo, "a.txt", "two", "second");
        assert_ne!(head_oid(&dir).unwrap(), before);

        reset_to_oid(&dir, &before).unwrap();
        assert_eq!(head_oid(&dir).unwrap(), before);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "one");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn discard_file_changes_returns_none_when_nothing_to_discard() {
        let dir = temp_dir("discard-noop");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "a.txt", "one", "init");

        assert_eq!(discard_file_changes(&dir, "a.txt").unwrap(), None);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn discard_tracked_edit_backs_up_and_restore_returns_it() {
        let dir = temp_dir("discard-tracked");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "a.txt", "one\n", "init");
        fs::write(dir.join("a.txt"), "one\nlocal-edit\n").unwrap();

        let backup_id = discard_file_changes(&dir, "a.txt").unwrap();
        assert!(backup_id.is_some());
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");

        restore_discard_backup(&dir, &backup_id.unwrap()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("a.txt")).unwrap(),
            "one\nlocal-edit\n"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn discard_untracked_file_backs_up_and_restore_recreates_it() {
        let dir = temp_dir("discard-untracked");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "a.txt", "one\n", "init");
        fs::write(dir.join("new.txt"), "brand new\n").unwrap();

        let backup_id = discard_file_changes(&dir, "new.txt").unwrap();
        assert!(backup_id.is_some(), "expected a backup for the untracked file");
        assert!(
            !dir.join("new.txt").exists(),
            "discard should still delete the untracked file from disk"
        );

        restore_discard_backup(&dir, &backup_id.unwrap()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("new.txt")).unwrap(),
            "brand new\n",
            "restore should recreate the untracked file with its original content"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn restore_discard_backup_errors_without_dropping_when_file_changed_since() {
        let dir = temp_dir("discard-restore-conflict");
        let repo = init_repo_with_identity(&dir);
        commit_file(&repo, "a.txt", "one\n", "init");
        fs::write(dir.join("a.txt"), "one\nlocal-edit\n").unwrap();

        let backup_id = discard_file_changes(&dir, "a.txt").unwrap().unwrap();
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "one\n");

        // Something else happens to the same file after the discard —
        // committed, so the stash's 3-way apply has to reconcile against a
        // genuinely different base than what it captured.
        commit_file(&repo, "a.txt", "one\nremote-edit\n", "unrelated update");

        let err = restore_discard_backup(&dir, &backup_id).unwrap_err();
        assert!(matches!(err, GitError::Message(_)));

        // The backup must still be there — nothing silently lost.
        let index = find_stash_index_by_oid_only(
            &mut open_repo(&dir).unwrap(),
            git2::Oid::from_str(&backup_id).unwrap(),
        );
        assert!(index.is_ok(), "backup stash entry should survive a failed restore");

        fs::remove_dir_all(&dir).ok();
    }
}