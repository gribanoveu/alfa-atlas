use std::path::{Path, PathBuf};

use git2::{Repository, Signature, Status, StatusOptions, StatusShow};

use crate::domain::git::{GitCommitSummary, GitError, GitFileStatus, GitStatusSnapshot};

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

fn validate_relative_path(path: &str) -> Result<&Path, GitError> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(GitError::InvalidPath(path.to_string()));
    }
    let p = Path::new(trimmed);
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
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
        let author = commit
            .author()
            .name()
            .unwrap_or("")
            .to_string();
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
