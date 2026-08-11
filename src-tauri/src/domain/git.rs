use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileStatus {
    pub path: String,
    /// Single-letter status: M, A, D, R, ?
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusSnapshot {
    pub staged: Vec<GitFileStatus>,
    pub unstaged: Vec<GitFileStatus>,
    /// Files with unresolved merge conflicts (status "U"). Computed from the
    /// index's conflict stages directly — independent of `merge_in_progress`,
    /// since the index can hold conflicted entries even without a MERGE_HEAD
    /// (e.g. an interrupted merge, or state staged outside a `git merge`).
    pub conflicted: Vec<GitFileStatus>,
    pub branch: Option<String>,
    /// Whether HEAD resolves to a commit (false on a brand-new, empty repo).
    pub has_commits: bool,
    /// Whether the current branch has an upstream remote-tracking branch configured.
    pub has_upstream: bool,
    /// Commits on HEAD not yet on the upstream (purely local — reflects the
    /// last fetch, not live network state). Only meaningful when `has_upstream`
    /// is true; `0` otherwise.
    pub ahead: usize,
    /// Whether a merge was left unfinished by a conflict (MERGE_HEAD present).
    pub merge_in_progress: bool,
}

/// A conflicted file's current on-disk content, including conflict markers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitConflictFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitSummary {
    pub hash: String,
    pub message: String,
    pub author: String,
    /// Unix timestamp (seconds).
    pub time: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PullMode {
    Merge,
    Rebase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitDiffScope {
    Staged,
    Unstaged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileDiff {
    pub original: String,
    pub modified: String,
    pub original_label: String,
    pub modified_label: String,
    pub is_binary: bool,
}

/// One contiguous run of lines attributed to the same commit — produced by
/// `infra::git_repo::blame` and surfaced to the AI harness as
/// `ToolResult::GitBlame`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBlameHunk {
    /// 1-indexed, inclusive.
    pub start_line: u32,
    /// 1-indexed, inclusive.
    pub end_line: u32,
    /// Short (7-char) commit hash.
    pub commit: String,
    pub author: String,
    /// ISO-8601 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
    pub authored_at: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchInfo {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
    /// Commits present on the branch's upstream but not yet pulled locally.
    /// `None` when the branch has no upstream (or is itself a remote branch).
    pub behind: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSyncStatus {
    pub ahead: usize,
    pub behind: usize,
}

/// One shelved (auto-stashed) set of tracked working-tree changes, tied to
/// the branch it was captured from. Backed by a real git2 stash entry
/// tagged with the `docflow-auto: ` message prefix — hand-made `git stash`
/// entries created outside the app are filtered out and never surfaced or
/// touched by any of the operations below.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStashEntry {
    /// Full stash commit oid (hex). Stable across index shifts, unlike
    /// libgit2's positional stash index, which shifts every time any stash
    /// is pushed or dropped.
    pub id: String,
    pub branch: String,
    /// Unix timestamp (seconds) — the stash commit's author time.
    pub created_at: i64,
    pub files_changed: usize,
}

/// Outcome of applying a shelf entry, either as part of an automatic
/// restore-on-checkout or a manual "Восстановить" action. Never implies the
/// entry was silently lost: `Conflict` and `Blocked` both leave the stash
/// entry in place so it stays visible and recoverable in the shelf list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum GitStashRestoreOutcome {
    /// Applied cleanly; the stash entry has already been dropped.
    Applied { entry: GitStashEntry },
    /// Conflict markers were written to the working tree and the index has
    /// conflicts (surfaced via the existing `GitStatusSnapshot.conflicted`).
    /// The stash entry was NOT dropped and remains in the shelf until the
    /// conflict is resolved.
    Conflict { entry: GitStashEntry },
    /// Restore was refused (e.g. the destination branch already has staged
    /// changes). Stash entry untouched.
    Blocked { entry: GitStashEntry, reason: String },
    /// More than one shelf entry exists for this branch — ambiguous, left
    /// for a manual pick via the shelf list instead of guessing which to
    /// apply.
    Skipped { count: usize },
}

/// Result of `checkout_branch`/`checkout_remote_branch` under the
/// auto-stash flow: tracked changes on the source branch are shelved
/// instead of blocking the switch, and any shelf entry for the destination
/// branch is auto-restored when unambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutOutcome {
    /// Some(..) if tracked changes on the source branch were auto-stashed.
    pub shelved: Option<GitStashEntry>,
    /// Some(..) if an auto-restore was attempted (or skipped as ambiguous)
    /// for the destination branch.
    pub restore: Option<GitStashRestoreOutcome>,
}

/// A progress update for a network-bound git operation (fetch/pull/push/
/// clone), emitted as Tauri events so the UI can show real progress instead
/// of a static "in progress" label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GitProgressEvent {
    Started { op: String },
    Transfer {
        op: String,
        received_objects: usize,
        total_objects: usize,
        received_bytes: usize,
        indexed_deltas: usize,
        total_deltas: usize,
    },
    Push {
        op: String,
        current: usize,
        total: usize,
        bytes: usize,
    },
    Finished { op: String },
}

/// SSH key stored in app settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SshKeySource {
    /// Key content pasted directly into the app.
    #[serde(rename = "keyContent")]
    KeyContent {
        #[serde(rename = "privateKey")]
        private_key: String,
    },
    /// Path to a key file on disk.
    #[serde(rename = "keyFile")]
    KeyFile { path: String },
}

/// A named SSH key configuration, optionally scoped to a host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshKeyConfig {
    pub name: String,
    /// Optional host pattern (e.g. "bitbucket.company.com") for auto-selection.
    pub host: Option<String>,
    pub source: SshKeySource,
    /// Optional passphrase for encrypted keys.
    pub passphrase: Option<String>,
}

/// Collection of git credentials stored by the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCredentials {
    #[serde(default)]
    pub ssh_keys: Vec<SshKeyConfig>,
    /// When true, accept any SSH host key on first connection (trust-on-first-use).
    /// When false, rely on ~/.ssh/known_hosts for host verification.
    #[serde(default = "default_true")]
    pub trust_all_ssh_host_keys: bool,
}

fn default_true() -> bool {
    true
}

impl Default for GitCredentials {
    fn default() -> Self {
        Self {
            ssh_keys: Vec::new(),
            trust_all_ssh_host_keys: true,
        }
    }
}

/// Status of the app-managed SSH key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppKeyStatus {
    pub exists: bool,
    pub public_key: String,
    /// Whether the private key is available (decryptable).
    pub private_key_available: bool,
    /// Whether the user imported an external key instead of using the generated one.
    pub is_imported: bool,
}

/// App-managed key configuration stored in ~/.atlas/key_config.json.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KeyConfig {
    pub public_key: String,
    /// Path to encrypted private key file, relative to settings dir.
    pub encrypted_private_key_path: String,
    /// True if the key was imported by the user (vs. generated by the app).
    pub is_imported: bool,
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("path is not a git repository: {0}")]
    NotARepository(String),
    #[error("failed to open repository: {0}")]
    Open(#[source] git2::Error),
    #[error("git operation failed: {0}")]
    Operation(#[source] git2::Error),
    #[error("commit message is empty")]
    EmptyMessage,
    #[error("nothing staged to commit")]
    NothingStaged,
    #[error(
        "git user.name / user.email are not configured; set them with git config user.name / user.email"
    )]
    MissingIdentity,
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("current branch has no upstream remote tracking branch")]
    NoUpstream,
    #[error("merge conflict; resolve conflicts manually and try again")]
    MergeConflict,
    #[error("rebase conflict; resolve conflicts manually and try again")]
    RebaseConflict,
    #[error("branch not found: {0}")]
    BranchNotFound(String),
    #[error("branch already exists: {0}")]
    BranchAlreadyExists(String),
    #[error("cannot delete the current branch")]
    CannotDeleteCurrentBranch,
    #[error("commit or discard tracked changes before switching branches")]
    CheckoutBlocked,
    #[error("clone failed: {0}")]
    CloneFailed(String),
    #[error("destination already exists: {0}")]
    DestinationExists(String),
    #[error("shelved changes not found: {0}")]
    StashNotFound(String),
    #[error("{0}")]
    Message(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_key_source_key_content_serialization() {
        let source = SshKeySource::KeyContent {
            private_key: "test-key-data".into(),
        };
        let json = serde_json::to_string(&source).unwrap();
        assert!(json.contains(r#""kind":"keyContent""#));

        let parsed: SshKeySource = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, source);
    }

    #[test]
    fn ssh_key_source_key_file_serialization() {
        let source = SshKeySource::KeyFile {
            path: "/home/user/.ssh/id_ed25519".into(),
        };
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, r#"{"kind":"keyFile","path":"/home/user/.ssh/id_ed25519"}"#);

        let parsed: SshKeySource = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, source);
    }

    #[test]
    fn ssh_key_config_full_serialization() {
        let config = SshKeyConfig {
            name: "Bitbucket Work".into(),
            host: Some("bitbucket.company.com".into()),
            source: SshKeySource::KeyContent {
                private_key: "secret".into(),
            },
            passphrase: Some("mypass".into()),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""name":"Bitbucket Work""#));
        assert!(json.contains(r#""host":"bitbucket.company.com""#));
        assert!(json.contains(r#""kind":"keyContent""#));
        assert!(json.contains(r#""passphrase":"mypass""#));

        let parsed: SshKeyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn ssh_key_config_minimal_serialization() {
        let config = SshKeyConfig {
            name: "Simple Key".into(),
            host: None,
            source: SshKeySource::KeyFile {
                path: "~/.ssh/id_rsa".into(),
            },
            passphrase: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""name":"Simple Key""#));
        assert!(json.contains(r#""host":null"#));
        assert!(json.contains(r#""passphrase":null"#));

        let parsed: SshKeyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn git_credentials_default_is_empty() {
        let creds = GitCredentials::default();
        assert!(creds.ssh_keys.is_empty());
    }

    #[test]
    fn git_credentials_serialization_empty() {
        let creds = GitCredentials::default();
        let json = serde_json::to_string(&creds).unwrap();
        assert_eq!(json, r#"{"sshKeys":[],"trustAllSshHostKeys":true}"#);

        // Deserializing the legacy format (without the field) defaults trust_all_ssh_host_keys to true.
        let parsed: GitCredentials = serde_json::from_str(r#"{"sshKeys":[]}"#).unwrap();
        assert!(parsed.ssh_keys.is_empty());
        assert!(parsed.trust_all_ssh_host_keys); // serde default = true
    }

    #[test]
    fn git_credentials_serialization_multiple_keys() {
        let creds = GitCredentials {
            ssh_keys: vec![
                SshKeyConfig {
                    name: "Key1".into(),
                    host: Some("github.com".into()),
                    source: SshKeySource::KeyContent {
                        private_key: "key1data".into(),
                    },
                    passphrase: None,
                },
                SshKeyConfig {
                    name: "Key2".into(),
                    host: None,
                    source: SshKeySource::KeyFile {
                        path: "/tmp/key".into(),
                    },
                    passphrase: Some("pw".into()),
                },
            ],
            trust_all_ssh_host_keys: true,
        };
        let json = serde_json::to_string(&creds).unwrap();
        assert!(json.contains("Key1"));
        assert!(json.contains("Key2"));
        assert!(json.contains("github.com"));
        assert!(json.contains("keyContent"));
        assert!(json.contains("keyFile"));

        let parsed: GitCredentials = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, creds);
    }

    #[test]
    fn git_credentials_deserialize_empty_json() {
        let parsed: GitCredentials = serde_json::from_str(r#"{}"#).unwrap();
        assert!(parsed.ssh_keys.is_empty());
    }

    #[test]
    fn git_credentials_deserialize_legacy_no_ssh_keys() {
        let parsed: GitCredentials = serde_json::from_str(r#"{"sshKeys":[]}"#).unwrap();
        assert!(parsed.ssh_keys.is_empty());
    }

    #[test]
    fn git_error_clone_failed_display() {
        let err = GitError::CloneFailed("test failure".into());
        assert_eq!(err.to_string(), "clone failed: test failure");
    }

    #[test]
    fn git_error_destination_exists_display() {
        let err = GitError::DestinationExists("/tmp/repo".into());
        assert_eq!(err.to_string(), "destination already exists: /tmp/repo");
    }
}
