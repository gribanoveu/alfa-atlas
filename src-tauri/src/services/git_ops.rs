use std::path::Path;

use crate::domain::git::{GitCommitSummary, GitError, GitStatusSnapshot};
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

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Repository;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("docflow-git-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        let repo = Repository::init(&dir).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Docflow Test").unwrap();
        config.set_str("user.email", "test@docflow.local").unwrap();
        dir
    }

    #[test]
    fn stage_commit_clears_status_and_appears_in_log() {
        let root = temp_repo();
        let root_str = root.to_str().unwrap();

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

        // Empty commit rejected.
        let err = commit(root_str, "docs: nothing").unwrap_err();
        assert!(matches!(err, GitError::NothingStaged));

        let err = commit(root_str, "   ").unwrap_err();
        assert!(matches!(err, GitError::EmptyMessage));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unstage_returns_file_to_changes() {
        let root = temp_repo();
        let root_str = root.to_str().unwrap();
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
}
