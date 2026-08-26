//! Query and delete OptMem raw log entries for the memory viewer UI.

use std::path::Path;

use crate::domain::memory_log::{MemoryLogDeleteRequest, MemoryLogFilter, MemoryLogPage, MemoryLogRow};
use crate::services::agent_memory::{self, AgentMemoryError, MemoryScope};

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 200;

#[derive(Debug, thiserror::Error)]
pub enum MemoryLogError {
    #[error("{0}")]
    Memory(#[from] AgentMemoryError),
}

struct TaggedRow {
    scope: MemoryScope,
    id: usize,
    date: String,
    text: String,
    store_path: String,
}

pub fn query(filter: &MemoryLogFilter) -> Result<MemoryLogPage, MemoryLogError> {
    let global_path = agent_memory::global_memory_dir()?;
    let global_path_str = global_path.display().to_string();

    let project_path = filter
        .repo_root
        .as_deref()
        .map(|root| agent_memory::project_memory_dir(Path::new(root)).display().to_string());

    let want_project = filter.scope.as_deref() != Some("global");
    let want_global = filter.scope.as_deref() != Some("project");

    let mut rows = Vec::new();

    if want_project {
        if let Some(repo_root) = filter.repo_root.as_deref() {
            let store_path = agent_memory::project_memory_dir(Path::new(repo_root))
                .display()
                .to_string();
            for entry in agent_memory::list_raw_entries(MemoryScope::Project, Path::new(repo_root))?
            {
                rows.push(TaggedRow {
                    scope: MemoryScope::Project,
                    id: entry.id,
                    date: entry.date,
                    text: entry.text,
                    store_path: store_path.clone(),
                });
            }
        }
    }

    if want_global {
        let store_path = global_path_str.clone();
        for entry in agent_memory::list_raw_entries(MemoryScope::Global, Path::new(""))? {
            rows.push(TaggedRow {
                scope: MemoryScope::Global,
                id: entry.id,
                date: entry.date,
                text: entry.text,
                store_path: store_path.clone(),
            });
        }
    }

    if let Some(search) = filter.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let needle = search.to_lowercase();
        rows.retain(|row| row.text.to_lowercase().contains(&needle));
    }

    rows.sort_by(|a, b| {
        b.id
            .cmp(&a.id)
            .then_with(|| a.scope.as_str().cmp(b.scope.as_str()))
    });

    let total = rows.len() as u32;
    let offset = filter.offset.unwrap_or(0);
    let limit = filter.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let page_rows: Vec<MemoryLogRow> = rows
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|row| MemoryLogRow {
            id: row.id as u32,
            scope: row.scope.as_str().to_string(),
            date: row.date,
            text: row.text,
            store_path: row.store_path,
        })
        .collect();

    Ok(MemoryLogPage {
        rows: page_rows,
        total,
        project_store_path: project_path,
        global_store_path: global_path_str,
    })
}

pub fn delete_entry(request: &MemoryLogDeleteRequest) -> Result<(), MemoryLogError> {
    let scope = parse_scope(&request.scope)?;
    let repo_root = request.repo_root.as_deref().unwrap_or("");
    if scope == MemoryScope::Project && repo_root.is_empty() {
        return Err(MemoryLogError::Memory(AgentMemoryError::Message(
            "project memory delete requires repoRoot".into(),
        )));
    }
    agent_memory::delete_log_entry(scope, Path::new(repo_root), request.id as usize)?;
    Ok(())
}

fn parse_scope(raw: &str) -> Result<MemoryScope, MemoryLogError> {
    MemoryScope::from_wire(raw).map_err(AgentMemoryError::Message).map_err(MemoryLogError::from)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Several tests in this module call this concurrently. A nanosecond
    /// timestamp alone does not reliably disambiguate them on a coarser
    /// system clock — two would share a directory and clobber each other.
    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_repo() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("memory-log-repo-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn query_returns_project_and_global_rows() {
        with_temp_home(|| {
            let repo = temp_repo();
            agent_memory::note(MemoryScope::Project, &repo, "project fact alpha").unwrap();
            agent_memory::note(MemoryScope::Global, &repo, "global fact beta").unwrap();

            let page = query(&MemoryLogFilter {
                repo_root: Some(repo.display().to_string()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(page.total, 2);
            assert!(page.project_store_path.is_some());
            assert!(!page.global_store_path.is_empty());

            let by_scope: Vec<_> = page.rows.iter().map(|r| r.scope.as_str()).collect();
            assert!(by_scope.contains(&"project"));
            assert!(by_scope.contains(&"global"));

            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn query_scope_global_skips_project() {
        with_temp_home(|| {
            let repo = temp_repo();
            agent_memory::note(MemoryScope::Project, &repo, "project only").unwrap();
            agent_memory::note(MemoryScope::Global, &repo, "global only").unwrap();

            let page = query(&MemoryLogFilter {
                scope: Some("global".to_string()),
                repo_root: Some(repo.display().to_string()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(page.total, 1);
            assert_eq!(page.rows[0].scope, "global");

            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn query_search_filters_text() {
        with_temp_home(|| {
            let repo = temp_repo();
            agent_memory::note(MemoryScope::Global, &repo, "User prefers Rust").unwrap();
            agent_memory::note(MemoryScope::Global, &repo, "Docs live under /docs").unwrap();

            let page = query(&MemoryLogFilter {
                scope: Some("global".to_string()),
                search: Some("rust".to_string()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(page.total, 1);
            assert!(page.rows[0].text.contains("Rust"));

            fs::remove_dir_all(&repo).ok();
        });
    }

    #[test]
    fn delete_entry_removes_global_row() {
        with_temp_home(|| {
            let repo = temp_repo();
            agent_memory::note(MemoryScope::Global, &repo, "to delete").unwrap();
            delete_entry(&MemoryLogDeleteRequest {
                scope: "global".to_string(),
                id: 0,
                repo_root: None,
            })
            .unwrap();
            let page = query(&MemoryLogFilter {
                scope: Some("global".to_string()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(page.total, 0);
            fs::remove_dir_all(&repo).ok();
        });
    }
}
