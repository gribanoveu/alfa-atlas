use std::path::Path;

use crate::domain::git::{
    GitBranchInfo, GitCommitSummary, GitDiffScope, GitError, GitFileDiff, GitStatusSnapshot,
    GitSyncStatus, PullMode,
};
use crate::infra::git_repo;

pub fn status(repo_root: &str) -> Result<GitStatusSnapshot, GitError> {
    git_repo::status(Path::new(repo_root))
}

pub fn stage(repo_root: &str, paths: &[String]) -> Result<(), GitError> {
    if paths.is_empty() {
        return Ok(());
    }
    git_repo::stage_paths(Path::new(repo_root), paths)
}

pub fn unstage(repo_root: &str, paths: &[String]) -> Result<(), GitError> {
    if paths.is_empty() {
        return Ok(());
    }
    git_repo::unstage_paths(Path::new(repo_root), paths)
}

pub fn commit(repo_root: &str, message: &str) -> Result<String, GitError> {
    git_repo::commit(Path::new(repo_root), message)
}

pub fn log(repo_root: &str, limit: usize) -> Result<Vec<GitCommitSummary>, GitError> {
    let limit = if limit == 0 { 20 } else { limit.min(100) };
    git_repo::log(Path::new(repo_root), limit)
}

pub fn pull(repo_root: &str, mode: PullMode) -> Result<(), GitError> {
    git_repo::pull(Path::new(repo_root), mode)
}

pub fn sync_status(repo_root: &str) -> Result<GitSyncStatus, GitError> {
    git_repo::sync_status(Path::new(repo_root))
}

pub fn reset_to_remote(repo_root: &str) -> Result<(), GitError> {
    git_repo::reset_to_remote(Path::new(repo_root))
}

pub fn push(repo_root: &str) -> Result<(), GitError> {
    git_repo::push(Path::new(repo_root))
}

pub fn file_diff(
    repo_root: &str,
    path: &str,
    scope: GitDiffScope,
) -> Result<GitFileDiff, GitError> {
    git_repo::file_diff(Path::new(repo_root), path, scope)
}

pub fn discard_file_changes(repo_root: &str, path: &str) -> Result<(), GitError> {
    git_repo::discard_file_changes(Path::new(repo_root), path)
}

pub fn list_branches(repo_root: &str) -> Result<Vec<GitBranchInfo>, GitError> {
    git_repo::list_local_branches(Path::new(repo_root))
}

pub fn create_branch(
    repo_root: &str,
    name: &str,
    discard_changes: bool,
) -> Result<(), GitError> {
    git_repo::create_branch(Path::new(repo_root), name, discard_changes)
}

pub fn checkout_branch(
    repo_root: &str,
    name: &str,
    discard_changes: bool,
) -> Result<(), GitError> {
    git_repo::checkout_branch(Path::new(repo_root), name, discard_changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::git::GitDiffScope;
    use git2::Repository;
    use std::fs;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("docflow-git-{prefix}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn init_repo(dir: &Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Docflow Test").unwrap();
        config.set_str("user.email", "test@docflow.local").unwrap();
        repo
    }

    fn git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn stage_commit_clears_status_and_appears_in_log() {
        let root = temp_dir("stage");
        let root_str = root.to_str().unwrap();
        init_repo(&root);

        fs::write(root.join("note.adoc"), "= Hello\n").unwrap();

        let snap = status(root_str).unwrap();
        assert!(snap.staged.is_empty());
        assert_eq!(snap.unstaged.len(), 1);
        assert_eq!(snap.unstaged[0].path, "note.adoc");
        assert_eq!(snap.unstaged[0].status, "?");

        stage(root_str, &["note.adoc".into()]).unwrap();
        let snap = status(root_str).unwrap();
        assert_eq!(snap.staged.len(), 1);
        assert_eq!(snap.staged[0].status, "A");
        assert!(snap.unstaged.is_empty());

        let hash = commit(root_str, "docs: add note").unwrap();
        assert_eq!(hash.len(), 7);

        let snap = status(root_str).unwrap();
        assert!(snap.staged.is_empty());
        assert!(snap.unstaged.is_empty());

        let commits = log(root_str, 10).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].message, "docs: add note");
        assert_eq!(commits[0].author, "Docflow Test");

        let err = commit(root_str, "docs: nothing").unwrap_err();
        assert!(matches!(err, GitError::NothingStaged));

        let err = commit(root_str, "   ").unwrap_err();
        assert!(matches!(err, GitError::EmptyMessage));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unstage_returns_file_to_changes() {
        let root = temp_dir("unstage");
        let root_str = root.to_str().unwrap();
        init_repo(&root);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        stage(root_str, &["a.txt".into()]).unwrap();
        commit(root_str, "init").unwrap();

        fs::write(root.join("a.txt"), "two\n").unwrap();
        stage(root_str, &["a.txt".into()]).unwrap();
        assert_eq!(status(root_str).unwrap().staged.len(), 1);

        unstage(root_str, &["a.txt".into()]).unwrap();
        let snap = status(root_str).unwrap();
        assert!(snap.staged.is_empty());
        assert_eq!(snap.unstaged.len(), 1);
        assert_eq!(snap.unstaged[0].status, "M");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pull_merge_and_reset_via_local_remote() {
        let remote = temp_dir("remote");
        init_repo(&remote);
        fs::write(remote.join("base.txt"), "base\n").unwrap();
        git(&remote, &["add", "."]);
        git(&remote, &["-c", "user.name=Docflow Test", "-c", "user.email=test@docflow.local", "commit", "-m", "init"]);
        // bare remote clone target
        let bare = temp_dir("bare");
        git(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &[
                "clone",
                "--bare",
                remote.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );

        let local = temp_dir("local");
        git(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &[
                "clone",
                bare.to_str().unwrap(),
                local.to_str().unwrap(),
            ],
        );
        git(&local, &["config", "user.name", "Docflow Test"]);
        git(&local, &["config", "user.email", "test@docflow.local"]);

        // Advance remote (via another clone)
        let other = temp_dir("other");
        git(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &[
                "clone",
                bare.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        git(&other, &["config", "user.name", "Docflow Test"]);
        git(&other, &["config", "user.email", "test@docflow.local"]);
        fs::write(other.join("remote.txt"), "from remote\n").unwrap();
        git(&other, &["add", "."]);
        git(&other, &["commit", "-m", "remote change"]);
        git(&other, &["push", "origin", "HEAD"]);

        let local_str = local.to_str().unwrap();
        pull(local_str, PullMode::Merge).unwrap();
        assert!(local.join("remote.txt").exists());

        // Local-only commit then reset to remote
        fs::write(local.join("local-only.txt"), "x\n").unwrap();
        git(&local, &["add", "."]);
        git(&local, &["commit", "-m", "local only"]);
        assert!(local.join("local-only.txt").exists());

        reset_to_remote(local_str).unwrap();
        assert!(!local.join("local-only.txt").exists());
        assert!(local.join("remote.txt").exists());

        fs::remove_dir_all(&remote).ok();
        fs::remove_dir_all(&bare).ok();
        fs::remove_dir_all(&local).ok();
        fs::remove_dir_all(&other).ok();
    }

    #[test]
    fn sync_status_reports_ahead_and_behind() {
        let remote = temp_dir("sync-remote");
        init_repo(&remote);
        fs::write(remote.join("base.txt"), "base\n").unwrap();
        git(&remote, &["add", "."]);
        git(
            &remote,
            &[
                "-c",
                "user.name=Docflow Test",
                "-c",
                "user.email=test@docflow.local",
                "commit",
                "-m",
                "init",
            ],
        );

        let bare = temp_dir("sync-bare");
        git(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &[
                "clone",
                "--bare",
                remote.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );

        let local = temp_dir("sync-local");
        git(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &[
                "clone",
                bare.to_str().unwrap(),
                local.to_str().unwrap(),
            ],
        );
        git(&local, &["config", "user.name", "Docflow Test"]);
        git(&local, &["config", "user.email", "test@docflow.local"]);

        let other = temp_dir("sync-other");
        git(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &[
                "clone",
                bare.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        git(&other, &["config", "user.name", "Docflow Test"]);
        git(&other, &["config", "user.email", "test@docflow.local"]);
        fs::write(other.join("remote.txt"), "from remote\n").unwrap();
        git(&other, &["add", "."]);
        git(&other, &["commit", "-m", "remote change"]);
        git(&other, &["push", "origin", "HEAD"]);

        let local_str = local.to_str().unwrap();

        let behind_only = sync_status(local_str).unwrap();
        assert_eq!(behind_only.ahead, 0);
        assert_eq!(behind_only.behind, 1);

        fs::write(local.join("local.txt"), "local\n").unwrap();
        git(&local, &["add", "."]);
        git(&local, &["commit", "-m", "local change"]);

        let diverged = sync_status(local_str).unwrap();
        assert_eq!(diverged.ahead, 1);
        assert_eq!(diverged.behind, 1);

        pull(local_str, PullMode::Merge).unwrap();

        let after_pull = sync_status(local_str).unwrap();
        assert_eq!(after_pull.behind, 0);
        assert!(after_pull.ahead >= 1);

        fs::remove_dir_all(&remote).ok();
        fs::remove_dir_all(&bare).ok();
        fs::remove_dir_all(&local).ok();
        fs::remove_dir_all(&other).ok();
    }

    #[test]
    fn file_diff_unstaged_shows_index_vs_workdir() {
        let root = temp_dir("diff-unstaged");
        let root_str = root.to_str().unwrap();
        init_repo(&root);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        stage(root_str, &["a.txt".into()]).unwrap();
        commit(root_str, "init").unwrap();

        fs::write(root.join("a.txt"), "two\n").unwrap();

        let diff = file_diff(root_str, "a.txt", GitDiffScope::Unstaged).unwrap();
        assert_eq!(diff.original_label, "Index");
        assert_eq!(diff.modified_label, "Working tree");
        assert_eq!(diff.original, "one\n");
        assert_eq!(diff.modified, "two\n");
        assert!(!diff.is_binary);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn file_diff_staged_shows_head_vs_index() {
        let root = temp_dir("diff-staged");
        let root_str = root.to_str().unwrap();
        init_repo(&root);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        stage(root_str, &["a.txt".into()]).unwrap();
        commit(root_str, "init").unwrap();

        fs::write(root.join("a.txt"), "two\n").unwrap();
        stage(root_str, &["a.txt".into()]).unwrap();

        let diff = file_diff(root_str, "a.txt", GitDiffScope::Staged).unwrap();
        assert_eq!(diff.original_label, "HEAD");
        assert_eq!(diff.modified_label, "Index");
        assert_eq!(diff.original, "one\n");
        assert_eq!(diff.modified, "two\n");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn file_diff_untracked_has_empty_original() {
        let root = temp_dir("diff-untracked");
        let root_str = root.to_str().unwrap();
        init_repo(&root);
        fs::write(root.join("new.txt"), "hello\n").unwrap();

        let diff = file_diff(root_str, "new.txt", GitDiffScope::Unstaged).unwrap();
        assert!(diff.original.is_empty());
        assert_eq!(diff.modified, "hello\n");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discard_file_changes_restores_to_head() {
        let root = temp_dir("discard");
        let root_str = root.to_str().unwrap();
        init_repo(&root);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        stage(root_str, &["a.txt".into()]).unwrap();
        commit(root_str, "init").unwrap();

        fs::write(root.join("a.txt"), "two\n").unwrap();
        stage(root_str, &["a.txt".into()]).unwrap();
        assert_eq!(status(root_str).unwrap().staged.len(), 1);

        discard_file_changes(root_str, "a.txt").unwrap();
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "one\n");
        let snap = status(root_str).unwrap();
        assert!(snap.staged.is_empty());
        assert!(snap.unstaged.is_empty());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discard_untracked_removes_file() {
        let root = temp_dir("discard-untracked");
        let root_str = root.to_str().unwrap();
        init_repo(&root);
        fs::write(root.join("new.txt"), "hello\n").unwrap();

        discard_file_changes(root_str, "new.txt").unwrap();
        assert!(!root.join("new.txt").exists());
        assert!(status(root_str).unwrap().unstaged.is_empty());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_create_and_checkout_branches() {
        let root = temp_dir("branches");
        let root_str = root.to_str().unwrap();
        init_repo(&root);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        stage(root_str, &["a.txt".into()]).unwrap();
        commit(root_str, "init").unwrap();

        let branches = list_branches(root_str).unwrap();
        assert_eq!(branches.len(), 1);
        assert!(branches.iter().any(|b| b.is_current));

        create_branch(root_str, "feature", false).unwrap();
        let branches = list_branches(root_str).unwrap();
        assert_eq!(branches.len(), 2);
        assert!(branches.iter().any(|b| b.name == "feature" && b.is_current));

        checkout_branch(root_str, "master", false).unwrap();
        let branches = list_branches(root_str).unwrap();
        let current = branches.iter().find(|b| b.is_current).unwrap();
        assert_eq!(current.name, "master");

        let err = create_branch(root_str, "feature", false).unwrap_err();
        assert!(matches!(err, GitError::BranchAlreadyExists(_)));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn checkout_blocked_with_tracked_changes_until_discard() {
        let root = temp_dir("checkout-blocked");
        let root_str = root.to_str().unwrap();
        init_repo(&root);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        stage(root_str, &["a.txt".into()]).unwrap();
        commit(root_str, "init").unwrap();

        create_branch(root_str, "feature", false).unwrap();
        fs::write(root.join("a.txt"), "feature edit\n").unwrap();

        let err = checkout_branch(root_str, "master", false).unwrap_err();
        assert!(matches!(err, GitError::CheckoutBlocked));
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "feature edit\n"
        );

        checkout_branch(root_str, "master", true).unwrap();
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "one\n");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn checkout_with_only_untracked_succeeds() {
        let root = temp_dir("checkout-untracked");
        let root_str = root.to_str().unwrap();
        init_repo(&root);
        fs::write(root.join("a.txt"), "one\n").unwrap();
        stage(root_str, &["a.txt".into()]).unwrap();
        commit(root_str, "init").unwrap();

        create_branch(root_str, "feature", false).unwrap();
        checkout_branch(root_str, "master", false).unwrap();

        fs::write(root.join("new.txt"), "untracked\n").unwrap();
        assert!(root.join("new.txt").exists());

        checkout_branch(root_str, "feature", false).unwrap();
        assert!(root.join("new.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("new.txt")).unwrap(),
            "untracked\n"
        );

        fs::remove_dir_all(&root).ok();
    }
}
