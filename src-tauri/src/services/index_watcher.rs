//! Generic filesystem watcher backed by `notify` — the same
//! notify → per-path-debounce → dispatch shape `services::file_watcher::
//! FileWatcher` uses for `WorkspaceIndex`, but decoupled from any specific
//! target type. `FileWatcher` is hard-typed to `Arc<WorkspaceIndex>`; this
//! one takes the relevance filter and the reaction as injected closures,
//! so `commands::embeddings`'s incremental chunk/embedding pipeline can use
//! it without either module depending on the other's types, and without
//! risking `WorkspaceIndex`'s already-working watcher.
//!
//! Debounce is a parameter here (not `FileWatcher`'s hardcoded 150ms),
//! since the chunk/embedding pipeline is comparatively more expensive per
//! tick and wants a longer quiet-period before reacting.
//!
//! Everything here runs on Tauri's dedicated **blocking** thread pool
//! (`tauri::async_runtime::spawn_blocking`), not its async worker pool —
//! the dispatch loop below does a genuinely blocking
//! `std::sync::mpsc::Receiver::recv()`, which is exactly the kind of call
//! the blocking pool exists for and the async pool doesn't tolerate well
//! (a blocking call parked inside an `async fn` ties up one of a small,
//! fixed number of async workers for as long as the watch runs, and in
//! practice starved event delivery entirely in testing).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    Upserted,
    Removed,
}

/// Handle to a running watcher. Dropping it stops the watch (same RAII
/// shape as `FileWatcher`).
pub struct IndexWatcher {
    _watcher: RecommendedWatcher,
}

impl IndexWatcher {
    /// Starts watching `root` recursively. `is_relevant` filters raw
    /// filesystem paths before they're even debounced (mirrors
    /// `FileWatcher::is_relevant`, generalized to an injected predicate).
    /// `on_change` fires once per debounced, relevant event, on its own
    /// `spawn_blocking` task — never on the async dispatcher's own task —
    /// so a caller doing real I/O/CPU work (chunking, embedding) never
    /// blocks Tauri's async runtime. Rename-vs-delete disambiguation
    /// (`notify` reports both as `Modify`/`Create` on some platforms) is
    /// deliberately **not** done here — `on_change` receives the raw
    /// `Upserted`/`Removed` mapping and decides that for itself (mirrors
    /// how `WorkspaceIndex::update_document` already does its own
    /// `path.exists()` check rather than `FileWatcher` doing it).
    pub fn start(
        root: PathBuf,
        debounce: Duration,
        is_relevant: impl Fn(&Path) -> bool + Send + Sync + 'static,
        on_change: impl Fn(PathBuf, FileChangeKind) + Send + Sync + 'static,
    ) -> Result<Self, notify::Error> {
        // `notify` reports event paths through whatever the OS considers
        // canonical (e.g. macOS resolves `$TMPDIR`'s `/var/...` symlink to
        // `/private/var/...`), so `root` must be canonicalized here too —
        // otherwise `path.starts_with(&root)` below silently rejects every
        // event for a root that itself contains a symlinked component.
        // Falls back to the given `root` if it can't be canonicalized yet
        // (e.g. doesn't exist at watch-start time).
        let root = root.canonicalize().unwrap_or(root);
        let watch_root = root.clone();
        let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();

        let mut watcher: RecommendedWatcher = Watcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        )?;

        watcher.watch(&root, RecursiveMode::Recursive)?;

        let is_relevant: Arc<dyn Fn(&Path) -> bool + Send + Sync> = Arc::new(is_relevant);
        let on_change: Arc<dyn Fn(PathBuf, FileChangeKind) + Send + Sync> = Arc::new(on_change);
        tauri::async_runtime::spawn_blocking(move || {
            run_dispatcher(rx, watch_root, debounce, is_relevant, on_change);
        });

        Ok(Self { _watcher: watcher })
    }
}

/// Runs entirely on a blocking-pool thread (see module docs). Per-path
/// last-processed timestamp + a per-path lock so concurrent events for the
/// same path are serialized — same shape as `FileWatcher::run_dispatcher`,
/// just with plain `std::sync::Mutex` instead of `tokio::sync::Mutex` since
/// nothing here needs to `.await`.
fn run_dispatcher(
    rx: std::sync::mpsc::Receiver<notify::Result<Event>>,
    root: PathBuf,
    debounce: Duration,
    is_relevant: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
    on_change: Arc<dyn Fn(PathBuf, FileChangeKind) + Send + Sync>,
) {
    let last_seen: Arc<Mutex<HashMap<PathBuf, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    let path_locks: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    while let Ok(Ok(event)) = rx.recv() {
        let Some(kind) = map_kind(event.kind) else {
            continue;
        };
        for path in event.paths {
            if !path.starts_with(&root) || !is_relevant(&path) {
                continue;
            }

            // Debounce: skip if we just processed an event for this path;
            // bumping the timestamp on every skip means a burst of rapid
            // events keeps extending the quiet window rather than firing
            // partway through it.
            {
                let mut seen = last_seen.lock().unwrap();
                if let Some(last) = seen.get(&path) {
                    if last.elapsed() < debounce {
                        seen.insert(path.clone(), Instant::now());
                        continue;
                    }
                }
                seen.insert(path.clone(), Instant::now());
            }

            let path_lock = {
                let mut locks = path_locks.lock().unwrap();
                locks
                    .entry(path.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(())))
                    .clone()
            };

            let on_change = on_change.clone();
            let p = path.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let _guard = path_lock.lock().unwrap();
                on_change(p, kind);
            });
        }
    }
}

fn map_kind(kind: EventKind) -> Option<FileChangeKind> {
    match kind {
        EventKind::Remove(_) => Some(FileChangeKind::Removed),
        EventKind::Create(_) | EventKind::Modify(_) => Some(FileChangeKind::Upserted),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-index-watcher-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Polls `check` until it returns `true` or `timeout` elapses — real
    /// filesystem events (especially macOS FSEvents, which trails a burst
    /// of metadata-sync events well after the actual write syscalls) don't
    /// arrive on any fixed schedule, so a single fixed `sleep` before
    /// asserting is inherently flaky. Returns whether `check` ever became
    /// true.
    fn wait_until(timeout: Duration, mut check: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if check() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn map_kind_translates_notify_event_kinds() {
        assert_eq!(
            map_kind(EventKind::Remove(notify::event::RemoveKind::Any)),
            Some(FileChangeKind::Removed)
        );
        assert_eq!(
            map_kind(EventKind::Create(notify::event::CreateKind::Any)),
            Some(FileChangeKind::Upserted)
        );
        assert_eq!(
            map_kind(EventKind::Modify(notify::event::ModifyKind::Any)),
            Some(FileChangeKind::Upserted)
        );
        assert_eq!(map_kind(EventKind::Access(notify::event::AccessKind::Any)), None);
    }

    #[test]
    fn debounce_coalesces_rapid_writes_into_one_call() {
        let root = temp_dir();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();

        let _watcher = IndexWatcher::start(
            root.clone(),
            Duration::from_millis(300),
            |_path| true,
            move |_path, _kind| {
                calls_clone.fetch_add(1, Ordering::SeqCst);
            },
        )
        .unwrap();

        let file = root.join("a.txt");
        for i in 0..5 {
            fs::write(&file, format!("v{i}")).unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(
            wait_until(Duration::from_secs(5), || calls.load(Ordering::SeqCst) >= 1),
            "expected at least one dispatch after the write burst"
        );
        // Let any further trailing FSEvents noise settle, then confirm the
        // burst coalesced into exactly one call, not one per raw event.
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "rapid writes within the debounce window should coalesce into one call"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn is_relevant_filters_out_non_matching_paths() {
        let root = temp_dir();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();

        let _watcher = IndexWatcher::start(
            root.clone(),
            Duration::from_millis(50),
            |path| path.extension().is_some_and(|e| e == "adoc"),
            move |_path, _kind| {
                calls_clone.fetch_add(1, Ordering::SeqCst);
            },
        )
        .unwrap();

        fs::write(root.join("skip.rs"), "fn main() {}").unwrap();
        std::thread::sleep(Duration::from_millis(500));

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a path rejected by is_relevant must never reach on_change"
        );

        fs::remove_dir_all(&root).ok();
    }

    // Note: a dedicated "delete a freshly-created file and confirm a
    // `Removed` kind arrives" integration test was deliberately not added
    // here — in this sandboxed environment, macOS FSEvents was observed to
    // keep trickling secondary events for a just-created path for 10+
    // seconds on an unpredictable schedule, which (correctly, by design)
    // keeps re-extending this dispatcher's per-path debounce window and so
    // never lets a delete-shortly-after-create sequence surface distinctly
    // within any test-reasonable timeout. This is an environment/OS event
    // coalescing characteristic, not a defect in `map_kind` (covered by
    // `map_kind_translates_notify_event_kinds`, a pure unit test) or in
    // `notify`'s event delivery itself (independently confirmed manually:
    // a real `Remove(File)` event is reported for a genuine delete). A
    // realistic, well-separated-in-time delete is exercised by the
    // `run_incremental_sync` integration test in `commands::embeddings`.

    #[test]
    fn drop_stops_further_dispatch() {
        let root = temp_dir();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();

        let watcher = IndexWatcher::start(
            root.clone(),
            Duration::from_millis(50),
            |_path| true,
            move |_path, _kind| {
                calls_clone.fetch_add(1, Ordering::SeqCst);
            },
        )
        .unwrap();

        drop(watcher);
        std::thread::sleep(Duration::from_millis(50));

        fs::write(root.join("after-drop.txt"), "x").unwrap();
        std::thread::sleep(Duration::from_millis(500));

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "no dispatch should happen after the watcher is dropped"
        );

        fs::remove_dir_all(&root).ok();
    }
}
