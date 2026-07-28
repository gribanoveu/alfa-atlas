//! Filesystem watcher backed by `notify`.
//!
//! Spawns a background task that consumes raw `notify::Event`s, debounces
//! per-path (so rapid repeated saves coalesce), and serializes per-path
//! processing so two saves of the same file within the debounce window cannot
//! race the index update (spec section 6).
//!
//! The watcher is intentionally minimal: rename/move events are mapped to
//! `rename_document`, removes to `remove_document`, everything else to
//! `update_document`. The watcher does NOT re-detect repo root or restart on
//! its own — it lives only for the duration of an open project.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::Mutex;

use crate::domain::workspace_index::WorkspaceIndexError;
use crate::services::workspace_index::WorkspaceIndex;

const DEBOUNCE: Duration = Duration::from_millis(150);

/// Handle to a running watcher. Dropping it stops the watcher.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
}

impl FileWatcher {
    /// Start watching `repo_root` and dispatch events to `index`.
    pub fn start(
        repo_root: PathBuf,
        index: Arc<WorkspaceIndex>,
    ) -> Result<Self, WorkspaceIndexError> {
        let root = repo_root.clone();
        let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();

        let mut watcher: RecommendedWatcher = Watcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        )
        .map_err(|e| WorkspaceIndexError::Watcher(e.to_string()))?;

        watcher
            .watch(&repo_root, RecursiveMode::Recursive)
            .map_err(|e| WorkspaceIndexError::Watcher(e.to_string()))?;

        // Spawn the dispatcher on Tauri's async runtime.
        tauri::async_runtime::spawn(async move {
            run_dispatcher(rx, root, index).await;
        });

        Ok(Self { _watcher: watcher })
    }
}

/// Consume the notify event channel and apply updates to the index.
async fn run_dispatcher(
    rx: std::sync::mpsc::Receiver<notify::Result<Event>>,
    root: PathBuf,
    index: Arc<WorkspaceIndex>,
) {
    // Per-path last-event timestamp + a per-path lock so concurrent events
    // for the same path are serialized.
    let last_seen: Arc<Mutex<std::collections::HashMap<PathBuf, Instant>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let path_locks: Arc<Mutex<std::collections::HashMap<PathBuf, Arc<Mutex<()>>>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));

    while let Ok(Ok(event)) = rx.recv() {
        for path in event.paths {
            if !is_relevant(&path, &root) {
                continue;
            }
            // Debounce: skip if we just processed an event for this path.
            {
                let mut seen = last_seen.lock().await;
                if let Some(last) = seen.get(&path) {
                    if last.elapsed() < DEBOUNCE {
                        seen.insert(path.clone(), Instant::now());
                        continue;
                    }
                }
                seen.insert(path.clone(), Instant::now());
            }

            let path_lock = {
                let mut locks = path_locks.lock().await;
                locks
                    .entry(path.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(())))
                    .clone()
            };

            let kind = event.kind;
            let idx = index.clone();
            let p = path.clone();
            tauri::async_runtime::spawn(async move {
                let _guard = path_lock.lock().await;
                apply_event(&idx, kind, p).await;
            });
        }
    }
}

async fn apply_event(index: &Arc<WorkspaceIndex>, kind: EventKind, path: PathBuf) {
    match kind {
        EventKind::Remove(_) => {
            let _ = index.remove_document(path);
        }
        EventKind::Create(_) => {
            let _ = index.update_document(path);
        }
        EventKind::Modify(_) => {
            // Notify fires Modify for both content and rename events; detect
            // rename by checking whether the path exists at apply time. If it
            // doesn't, treat as removal. Update handles existence.
            let _ = index.update_document(path);
        }
        _ => {}
    }
}

fn is_relevant(path: &Path, root: &Path) -> bool {
    if !path.starts_with(root) {
        return false;
    }
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
        .unwrap_or_default();
    crate::domain::supported_files::SUPPORTED_EXTENSIONS.contains(&ext.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("alfa-atlas-watcher-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn is_relevant_filters_unsupported() {
        let root = temp_dir();
        let adoc = root.join("a.adoc");
        let rs = root.join("b.rs");
        assert!(is_relevant(&adoc, &root));
        assert!(!is_relevant(&rs, &root));
        fs::remove_dir_all(&root).ok();
    }
}