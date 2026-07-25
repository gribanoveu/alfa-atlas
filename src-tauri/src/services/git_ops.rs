use std::path::Path;

use crate::domain::git::{GitCommitSummary, GitError, GitStatusSnapshot, PullMode};
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

pub fn reset_to_remote(repo_root: &str) -> Result<(), GitError> {
    git_repo::reset_to_remote(Path::new(repo_root))
}

pub fn push(repo_root: &str) -> Result<(), GitError> {
    git_repo::push(Path::new(repo_root))
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
