use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tauri::{AppHandle, Emitter, State};

use crate::services::embedding_state::FullSyncActiveSlot;
use crate::domain::git::{
    AppKeyStatus, CheckoutOutcome, GitBranchInfo, GitCommitSummary, GitConflictFile,
    GitCredentials, GitDiffScope, GitFileDiff, GitFileStatus, GitPhase, GitProgressEvent,
    GitResetMode,
    GitStashEntry, GitStashRestoreOutcome, GitStatusSnapshot, GitSyncStatus, PullMode,
};
use crate::domain::project_config::ProbeResult;
use crate::services::{git_clone, git_credentials, git_ops};

/// Rejects a branch-switching command while a full `embedding_sync` walk is
/// in flight — see `FullSyncActiveSlot`'s doc comment for why this only
/// covers the first/full sync, not incremental watcher ticks.
fn reject_if_full_sync_active(flag: &FullSyncActiveSlot) -> Result<(), String> {
    if flag.load(Ordering::Acquire) {
        return Err(
            "Идёт первичная синхронизация эмбеддингов. Дождитесь её завершения, затем переключите ветку."
                .to_string(),
        );
    }
    Ok(())
}

const GIT_PROGRESS_EVENT: &str = "git://progress";
/// libgit2's `transfer_progress` fires very frequently on large repos —
/// throttle emissions to the UI so we don't flood the event channel.
const GIT_PROGRESS_THROTTLE: Duration = Duration::from_millis(100);

/// How long a network operation may stay silent — not a single progress event
/// — before the command stops waiting for it.
///
/// This is not a cap on duration: a large clone emits events continuously and
/// may run for as long as it needs. The threshold sits above the UI's own
/// "server is not responding" hint (one minute) so the user gets to cancel
/// first; the timeout is for the case where cancelling no longer works,
/// because the thread is stuck inside a blocking libgit2 call and never
/// reaches the callback where cancellation is checked.
const NETWORK_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// How often the watchdog wakes up to compare the silence against the threshold.
const NETWORK_WATCHDOG_TICK: Duration = Duration::from_secs(5);

/// What a timed-out network operation reports. What matters to the user is not
/// that some timeout elapsed, but what to go and check.
pub const NETWORK_TIMEOUT_MESSAGE: &str =
    "Сервер не отвечает более двух минут — операция прервана. Проверьте доступ к хосту и VPN.";

/// The last sign of life from a network operation: the worker thread stamps it
/// on every libgit2 callback, and the watchdog reads how long the silence has
/// lasted. `AtomicU64` rather than `Mutex<Instant>` because the write happens
/// in a callback invoked from inside libgit2's C code, which is no place for a
/// lock.
struct ActivityClock {
    start: Instant,
    last_millis: AtomicU64,
}

impl ActivityClock {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            last_millis: AtomicU64::new(0),
        }
    }

    fn touch(&self) {
        self.last_millis
            .store(self.start.elapsed().as_millis() as u64, Ordering::Relaxed);
    }

    fn idle_for(&self) -> Duration {
        let last = Duration::from_millis(self.last_millis.load(Ordering::Relaxed));
        self.start.elapsed().saturating_sub(last)
    }
}

/// How waiting for a network operation ended.
enum NetOpOutcome<T> {
    /// The operation answered — with success or with a libgit2 failure.
    Answered(Result<T, String>),
    /// The operation went silent. The worker thread is still there: it cannot
    /// be stopped from the outside, so it stays wedged until the app exits
    /// while the command hands control back to the UI without it.
    TimedOut,
}

impl<T> NetOpOutcome<T> {
    fn or_timeout_error(self) -> Result<T, String> {
        match self {
            NetOpOutcome::Answered(result) => result,
            NetOpOutcome::TimedOut => Err(NETWORK_TIMEOUT_MESSAGE.to_string()),
        }
    }
}

/// Runs a blocking network git operation with a progress emitter, and stops
/// waiting for it once it has been silent for `NETWORK_IDLE_TIMEOUT`.
///
/// `Started`/`Finished` are emitted here rather than in every command: the UI
/// clears its progress state on `Finished`, so the timeout path has to send it
/// too — otherwise a frozen percentage stays on screen forever.
async fn run_network_op<T, F>(op: &str, app: AppHandle, work: F) -> NetOpOutcome<T>
where
    T: Send + 'static,
    F: FnOnce(&mut dyn FnMut(GitProgressEvent)) -> Result<T, String> + Send + 'static,
{
    let activity = Arc::new(ActivityClock::new());
    let (tx, mut rx) = tokio::sync::oneshot::channel();

    let worker_op = op.to_string();
    let worker_app = app.clone();
    let worker_activity = Arc::clone(&activity);
    tauri::async_runtime::spawn_blocking(move || {
        let mut emit = progress_emitter(worker_app, worker_activity);
        emit(GitProgressEvent::Started {
            op: worker_op.clone(),
        });
        let result = work(&mut emit);
        emit(GitProgressEvent::Finished { op: worker_op });
        // The receiver may already have given up on the timeout, in which case
        // there is simply nobody to send to — not an error.
        let _ = tx.send(result);
    });

    loop {
        match tokio::time::timeout(NETWORK_WATCHDOG_TICK, &mut rx).await {
            Ok(Ok(result)) => return NetOpOutcome::Answered(result),
            // The sender was dropped without answering: a panic in the worker.
            Ok(Err(_)) => {
                return NetOpOutcome::Answered(Err(format!(
                    "операция git ({op}) завершилась аварийно"
                )))
            }
            Err(_) if activity.idle_for() >= NETWORK_IDLE_TIMEOUT => {
                let _ = app.emit(
                    GIT_PROGRESS_EVENT,
                    &GitProgressEvent::Finished { op: op.to_string() },
                );
                return NetOpOutcome::TimedOut;
            }
            Err(_) => {}
        }
    }
}

/// Builds a progress callback that forwards `GitProgressEvent`s to the
/// frontend over `GIT_PROGRESS_EVENT`, throttled so only ~10/sec reach the
/// UI regardless of how often libgit2 invokes the underlying callback.
/// `Started`/`Finished` are one-off lifecycle markers rather than a
/// high-frequency tick, so they always bypass the throttle.
fn progress_emitter(app: AppHandle, activity: Arc<ActivityClock>) -> impl FnMut(GitProgressEvent) {
    let mut last = Instant::now()
        .checked_sub(GIT_PROGRESS_THROTTLE)
        .unwrap_or_else(Instant::now);
    move |event: GitProgressEvent| {
        // Stamped before throttling: what the watchdog needs to know is that the
        // operation is alive, even when this particular event never reaches the UI.
        activity.touch();
        let now = Instant::now();
        // Phase transitions are one-off and few, and throttling them away
        // would hide exactly the information that makes a stall diagnosable.
        // `Remote` is the exception: it carries the server's own sideband
        // counter, which ticks as fast as any transfer callback.
        let is_lifecycle_marker = matches!(
            event,
            GitProgressEvent::Started { .. }
                | GitProgressEvent::Finished { .. }
                | GitProgressEvent::Phase {
                    phase: GitPhase::Connecting | GitPhase::Authenticating | GitPhase::HostKey,
                    ..
                }
        );
        if is_lifecycle_marker || now.duration_since(last) >= GIT_PROGRESS_THROTTLE {
            last = now;
            let _ = app.emit(GIT_PROGRESS_EVENT, &event);
        }
    }
}

#[tauri::command]
pub fn git_status(repo_root: String) -> Result<GitStatusSnapshot, String> {
    git_ops::status(&repo_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_stage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    git_ops::stage(&repo_root, &paths).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_unstage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    git_ops::unstage(&repo_root, &paths).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_commit(repo_root: String, message: String) -> Result<String, String> {
    git_ops::commit(&repo_root, &message).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_log(repo_root: String, limit: Option<usize>) -> Result<Vec<GitCommitSummary>, String> {
    git_ops::log(&repo_root, limit.unwrap_or(20)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_unpushed_commits(
    repo_root: String,
    limit: Option<usize>,
) -> Result<Vec<GitCommitSummary>, String> {
    git_ops::unpushed_commits(&repo_root, limit.unwrap_or(50)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn git_incoming_commits(
    repo_root: String,
    limit: Option<usize>,
) -> Result<Vec<GitCommitSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_ops::incoming_commits(&repo_root, limit.unwrap_or(50)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn git_drop_unpushed_from(
    repo_root: String,
    commit_hash: String,
    mode: GitResetMode,
) -> Result<(), String> {
    git_ops::drop_unpushed_from(&repo_root, &commit_hash, mode).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_drop_all_unpushed(repo_root: String, mode: GitResetMode) -> Result<(), String> {
    git_ops::drop_all_unpushed(&repo_root, mode).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_move_unpushed_to_new_branch(repo_root: String, new_name: String) -> Result<(), String> {
    git_ops::move_unpushed_to_new_branch(&repo_root, &new_name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_move_unpushed_to_branch(
    repo_root: String,
    target_branch: String,
) -> Result<(), String> {
    git_ops::move_unpushed_to_branch(&repo_root, &target_branch).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn git_pull(repo_root: String, mode: PullMode, app: AppHandle) -> Result<(), String> {
    run_network_op("pull", app, move |emit| {
        git_ops::pull(&repo_root, mode, Some(emit)).map_err(|e| e.to_string())
    })
    .await
    .or_timeout_error()
}

#[tauri::command]
pub async fn git_reset_to_remote(repo_root: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_ops::reset_to_remote(&repo_root).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn git_conflict_file_content(
    repo_root: String,
    path: String,
) -> Result<GitConflictFile, String> {
    git_ops::conflict_file_content(&repo_root, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_resolve_conflict(
    repo_root: String,
    path: String,
    content: String,
) -> Result<(), String> {
    git_ops::resolve_conflict(&repo_root, &path, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_finish_merge(repo_root: String) -> Result<String, String> {
    git_ops::finish_merge(&repo_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_abort_merge(repo_root: String) -> Result<(), String> {
    git_ops::abort_merge(&repo_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn git_sync_status(repo_root: String) -> Result<GitSyncStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_ops::sync_status(&repo_root).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_push(repo_root: String, app: AppHandle) -> Result<(), String> {
    run_network_op("push", app, move |emit| {
        git_ops::push(&repo_root, Some(emit)).map_err(|e| e.to_string())
    })
    .await
    .or_timeout_error()
}

#[tauri::command]
pub fn git_file_diff(
    repo_root: String,
    path: String,
    scope: GitDiffScope,
) -> Result<GitFileDiff, String> {
    git_ops::file_diff(&repo_root, &path, scope).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_commit_files(
    repo_root: String,
    commit_hash: String,
) -> Result<Vec<GitFileStatus>, String> {
    git_ops::commit_files(&repo_root, &commit_hash).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_commit_file_diff(
    repo_root: String,
    commit_hash: String,
    path: String,
) -> Result<GitFileDiff, String> {
    git_ops::commit_file_diff(&repo_root, &commit_hash, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_discard_file_changes(repo_root: String, path: String) -> Result<Option<String>, String> {
    git_ops::discard_file_changes(&repo_root, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_restore_discard_backup(repo_root: String, backup_id: String) -> Result<(), String> {
    git_ops::restore_discard_backup(&repo_root, &backup_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_undo_commit(repo_root: String, commit_hash: String) -> Result<(), String> {
    git_ops::undo_commit(&repo_root, &commit_hash).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_create_branch_at_oid(repo_root: String, name: String, oid: String) -> Result<(), String> {
    git_ops::create_branch_at_oid(&repo_root, &name, &oid).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_reset_to_oid(repo_root: String, oid: String) -> Result<(), String> {
    git_ops::reset_to_oid(&repo_root, &oid).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_head_oid(repo_root: String) -> Result<String, String> {
    git_ops::head_oid(&repo_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_apply_diff_content(
    repo_root: String,
    path: String,
    scope: GitDiffScope,
    content: String,
) -> Result<(), String> {
    git_ops::apply_diff_content(&repo_root, &path, scope, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_list_branches(repo_root: String) -> Result<Vec<GitBranchInfo>, String> {
    git_ops::list_branches(&repo_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn git_fetch_branches(repo_root: String, app: AppHandle) -> Result<(), String> {
    run_network_op("fetch", app, move |emit| {
        git_ops::fetch_branches(&repo_root, Some(emit)).map_err(|e| e.to_string())
    })
    .await
    .or_timeout_error()
}

#[tauri::command]
pub fn git_create_branch(
    repo_root: String,
    name: String,
    discard_changes: bool,
    full_sync_active: State<'_, Arc<FullSyncActiveSlot>>,
) -> Result<(), String> {
    reject_if_full_sync_active(&full_sync_active)?;
    git_ops::create_branch(&repo_root, &name, discard_changes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_checkout_branch(
    repo_root: String,
    name: String,
    discard_changes: bool,
    full_sync_active: State<'_, Arc<FullSyncActiveSlot>>,
) -> Result<CheckoutOutcome, String> {
    reject_if_full_sync_active(&full_sync_active)?;
    git_ops::checkout_branch(&repo_root, &name, discard_changes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_delete_branch(repo_root: String, name: String) -> Result<(), String> {
    git_ops::delete_branch(&repo_root, &name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_checkout_remote_branch(
    repo_root: String,
    name: String,
    discard_changes: bool,
    full_sync_active: State<'_, Arc<FullSyncActiveSlot>>,
) -> Result<CheckoutOutcome, String> {
    reject_if_full_sync_active(&full_sync_active)?;
    git_ops::checkout_remote_branch(&repo_root, &name, discard_changes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_stash_list(repo_root: String) -> Result<Vec<GitStashEntry>, String> {
    git_ops::stash_list(&repo_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_stash_apply(
    repo_root: String,
    stash_id: String,
) -> Result<GitStashRestoreOutcome, String> {
    git_ops::stash_apply(&repo_root, &stash_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_stash_drop(repo_root: String, stash_id: String) -> Result<(), String> {
    git_ops::stash_drop(&repo_root, &stash_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn git_get_credentials() -> Result<GitCredentials, String> {
    git_credentials::load_credentials()
}

#[tauri::command]
pub fn git_save_credentials(credentials: GitCredentials) -> Result<(), String> {
    git_credentials::save_credentials(credentials)
}

#[tauri::command]
pub fn git_get_key_status() -> Result<AppKeyStatus, String> {
    git_credentials::get_app_key_status()
}

#[tauri::command]
pub fn git_generate_key() -> Result<AppKeyStatus, String> {
    crate::infra::key_management::generate_and_store_key_app()
}

#[tauri::command]
pub fn git_import_key(source_path: String) -> Result<AppKeyStatus, String> {
    let path = std::path::Path::new(&source_path);
    crate::infra::key_management::import_key_file(path)
}

/// Clones the frontend has asked to abandon, keyed by the id it generated for
/// the run. Registered in `lib.rs`; entries are removed by the clone task
/// itself once it observes them (or finishes).
#[derive(Default)]
pub struct CloneCancellations(DashMap<String, ()>);

impl CloneCancellations {
    pub fn new() -> Self {
        Self::default()
    }

    fn is_cancelled(&self, clone_id: &str) -> bool {
        self.0.contains_key(clone_id)
    }

    fn clear(&self, clone_id: &str) {
        self.0.remove(clone_id);
    }
}

#[tauri::command]
pub async fn git_clone(
    url: String,
    destination: String,
    clone_id: String,
    cancellations: State<'_, Arc<CloneCancellations>>,
    app: AppHandle,
) -> Result<ProbeResult, String> {
    let cancellations = Arc::clone(&cancellations);
    let registry = Arc::clone(&cancellations);
    let timed_out_id = clone_id.clone();

    let outcome = run_network_op("clone", app, move |emit| {
        let cancel_id = clone_id.clone();
        let cancel_registry = Arc::clone(&cancellations);
        let is_cancelled = move || cancel_registry.is_cancelled(&cancel_id);

        let cloned = git_clone::clone_repository(&url, &destination, Some(emit), Some(&is_cancelled));

        cancellations.clear(&clone_id);
        cloned?;

        // After cloning, probe the repo to find docs root candidates.
        // The frontend will show ConfirmOpenProjectModal for the user to pick.
        let dest_path = std::path::Path::new(&destination);
        let canonical = dest_path
            .canonicalize()
            .map(crate::domain::paths::strip_verbatim)
            .unwrap_or_else(|_| dest_path.to_path_buf());
        let repo_root = crate::infra::git_repo::discover_repo_root(&canonical);
        let repo_root_str = repo_root.to_string_lossy().into_owned();

        crate::services::project_open::probe_open_path(&repo_root_str).map_err(|e| e.to_string())
    })
    .await;

    match outcome {
        NetOpOutcome::Answered(result) => result,
        NetOpOutcome::TimedOut => {
            // The thread is still inside a blocking call and may wake up long
            // after we stopped waiting for it. The cancellation flag is what
            // makes it drop the clone and remove the half-fetched directory
            // then, instead of finishing it behind the back of a user who
            // closed the dialog minutes ago. Nothing will ever clear this
            // entry — the price of not being able to kill the thread.
            registry.0.insert(timed_out_id, ());
            Err(NETWORK_TIMEOUT_MESSAGE.to_string())
        }
    }
}

/// Asks an in-flight `git_clone` to stop. The clone thread notices at its next
/// libgit2 callback; if it is wedged inside a blocking syscall it may never
/// notice at all, which is why the frontend stops waiting on its own rather
/// than treating this as a handshake.
#[tauri::command]
pub fn git_clone_cancel(clone_id: String, cancellations: State<'_, Arc<CloneCancellations>>) {
    cancellations.0.insert(clone_id, ());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_if_full_sync_active_errs_while_a_full_sync_is_running() {
        let flag = FullSyncActiveSlot::new(true);
        assert!(reject_if_full_sync_active(&flag).is_err());
    }

    #[test]
    fn reject_if_full_sync_active_allows_checkout_when_idle() {
        let flag = FullSyncActiveSlot::new(false);
        assert!(reject_if_full_sync_active(&flag).is_ok());
    }

    #[test]
    fn activity_clock_counts_silence_from_the_last_sign_of_life() {
        let clock = ActivityClock::new();
        std::thread::sleep(Duration::from_millis(30));
        // Before the first stamp, the whole run counts as silence — otherwise an
        // operation that never emitted anything would look alive.
        assert!(clock.idle_for() >= Duration::from_millis(25));

        clock.touch();
        assert!(clock.idle_for() < Duration::from_millis(25));
    }

    #[test]
    fn a_timed_out_operation_tells_the_user_what_to_check() {
        let outcome: NetOpOutcome<()> = NetOpOutcome::TimedOut;
        assert_eq!(outcome.or_timeout_error(), Err(NETWORK_TIMEOUT_MESSAGE.to_string()));
    }

    #[test]
    fn an_answered_operation_passes_its_own_result_through() {
        let failed: NetOpOutcome<()> = NetOpOutcome::Answered(Err("хост не найден".into()));
        assert_eq!(failed.or_timeout_error(), Err("хост не найден".to_string()));
        assert_eq!(NetOpOutcome::Answered(Ok(7)).or_timeout_error(), Ok(7));
    }
}
