//! Fixtures shared by every tool module's tests. The wrappers all go
//! through `execute_tool` rather than the tool functions directly, so each
//! test exercises the real entry point — allowlist check included — while
//! still reading like a plain call.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::ai_tools::{
    CreateDirectoryArgs, DeleteDirectoryArgs, DeleteFileArgs, EditFileArgs, FileEdit, ListFilesArgs,
    MoveArgs, ReadFileArgs, ToolCall, ToolError, ToolFileEntry, ToolResult, ToolScope, WriteFileArgs,
};
use crate::domain::project_config::UpdatedReference;
use crate::services::workspace_index::WorkspaceIndex;

use super::{execute_tool, EmbeddingDeps};

/// Builds a `repo_root/docs/...` + `repo_root/src/...` fixture and
/// returns `(repo_root, docs_root)`, both canonicalized.
static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique, not-yet-created temp path. The counter matters as much as the
/// timestamp: these tests run in parallel, and a coarse system clock hands
/// several of them the same nanosecond reading.
pub(crate) fn fixture_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("alfa-atlas-ai-tools-{prefix}{nanos}-{n}"))
}

pub(crate) fn fixture_repo() -> (PathBuf, PathBuf) {
    let repo = fixture_dir("");
    let docs = repo.join("docs");
    let src = repo.join("src");
    fs::create_dir_all(&docs).unwrap();
    fs::create_dir_all(&src).unwrap();
    fs::write(docs.join("intro.adoc"), "= Intro\n").unwrap();
    fs::write(docs.join("script.py"), "print('unsupported ext')\n").unwrap();
    fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();

    let repo = repo.canonicalize().unwrap();
    let docs = docs.canonicalize().unwrap();
    (repo, docs)
}

/// Calls `execute_tool` for `ReadFile` and unwraps the expected
/// `ToolResult::File` shape, so tests read like the plain `read_file`
/// calls they replaced while still exercising the real public entry
/// point (allowlist check included).
pub(crate) fn read(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
    match execute_tool(
        scope,
        ToolCall::ReadFile(ReadFileArgs {
            path: path.to_string(),
            start_line: None,
            end_line: None,
        }),
        &EmbeddingDeps::empty(),
        &[],
    )? {
        ToolResult::File { content, .. } => Ok(content),
        other => panic!("expected ToolResult::File, got {other:?}"),
    }
}

/// Like `read`, but returns the full `ToolResult` so range/total-line
/// metadata is inspectable, and takes an explicit line range.
pub(crate) fn read_range(
    scope: &ToolScope,
    path: &str,
    start_line: Option<u32>,
    end_line: Option<u32>,
) -> Result<ToolResult, ToolError> {
    execute_tool(
        scope,
        ToolCall::ReadFile(ReadFileArgs {
            path: path.to_string(),
            start_line,
            end_line,
        }),
        &EmbeddingDeps::empty(),
        &[],
    )
}

pub(crate) fn list(scope: &ToolScope, path: Option<&str>) -> Result<Vec<ToolFileEntry>, ToolError> {
    list_scoped(scope, path, None, None)
}

pub(crate) fn list_scoped(
    scope: &ToolScope,
    path: Option<&str>,
    depth: Option<u32>,
    pattern: Option<&str>,
) -> Result<Vec<ToolFileEntry>, ToolError> {
    match execute_tool(
        scope,
        ToolCall::ListFiles(ListFilesArgs {
            path: path.map(str::to_string),
            depth,
            pattern: pattern.map(str::to_string),
        }),
        &EmbeddingDeps::empty(),
        &[],
    )? {
        ToolResult::FileList(entries) => Ok(entries),
        other => panic!("expected ToolResult::FileList, got {other:?}"),
    }
}

pub(crate) fn write(scope: &ToolScope, path: &str, content: &str) -> Result<String, ToolError> {
    match execute_tool(
        scope,
        ToolCall::WriteFile(WriteFileArgs {
            path: path.to_string(),
            content: content.to_string(),
        }),
        &EmbeddingDeps::empty(),
        &[],
    )? {
        ToolResult::FileWritten { path, .. } => Ok(path),
        other => panic!("expected ToolResult::FileWritten, got {other:?}"),
    }
}

pub(crate) fn edit(scope: &ToolScope, path: &str, edits: Vec<(&str, &str)>) -> Result<String, ToolError> {
    match execute_tool(
        scope,
        ToolCall::EditFile(EditFileArgs {
            path: path.to_string(),
            edits: edits
                .into_iter()
                .map(|(old, new)| FileEdit { old: old.to_string(), new: new.to_string() })
                .collect(),
        }),
        &EmbeddingDeps::empty(),
        &[],
    )? {
        ToolResult::FileEdited { path, .. } => Ok(path),
        other => panic!("expected ToolResult::FileEdited, got {other:?}"),
    }
}

pub(crate) fn create_dir(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
    create_dir_with_template(scope, path, None).map(|(path, _, _)| path)
}

pub(crate) fn create_dir_with_template(
    scope: &ToolScope,
    path: &str,
    template: Option<&str>,
) -> Result<(String, Option<String>, Vec<String>), ToolError> {
    match execute_tool(
        scope,
        ToolCall::CreateDirectory(CreateDirectoryArgs {
            path: path.to_string(),
            template: template.map(str::to_string),
        }),
        &EmbeddingDeps::empty(),
        &[],
    )? {
        ToolResult::DirectoryCreated {
            path,
            template,
            created_files,
        } => Ok((path, template, created_files)),
        other => panic!("expected ToolResult::DirectoryCreated, got {other:?}"),
    }
}

pub(crate) fn delete(scope: &ToolScope, path: &str) -> Result<String, ToolError> {
    match execute_tool(
        scope,
        ToolCall::DeleteFile(DeleteFileArgs { path: path.to_string() }),
        &EmbeddingDeps::empty(),
        &[],
    )? {
        ToolResult::FileDeleted { path, .. } => Ok(path),
        other => panic!("expected ToolResult::FileDeleted, got {other:?}"),
    }
}

pub(crate) fn delete_dir(scope: &ToolScope, path: &str, recursive: Option<bool>) -> Result<String, ToolError> {
    match execute_tool(
        scope,
        ToolCall::DeleteDirectory(DeleteDirectoryArgs { path: path.to_string(), recursive }),
        &EmbeddingDeps::empty(),
        &[],
    )? {
        ToolResult::DirectoryDeleted { path } => Ok(path),
        other => panic!("expected ToolResult::DirectoryDeleted, got {other:?}"),
    }
}

pub(crate) fn move_it(
    scope: &ToolScope,
    path: &str,
    new_path: &str,
) -> Result<(String, String, Vec<UpdatedReference>), ToolError> {
    move_it_with_deps(scope, path, new_path, &EmbeddingDeps::empty())
}

pub(crate) fn move_it_with_deps(
    scope: &ToolScope,
    path: &str,
    new_path: &str,
    deps: &EmbeddingDeps,
) -> Result<(String, String, Vec<UpdatedReference>), ToolError> {
    match execute_tool(
        scope,
        ToolCall::Move(MoveArgs { path: path.to_string(), new_path: new_path.to_string() }),
        deps,
        &[],
    )? {
        ToolResult::Moved { from, to, updated_files } => Ok((from, to, updated_files)),
        other => panic!("expected ToolResult::Moved, got {other:?}"),
    }
}

/// A `WorkspaceIndex` built from a real walk of `repo_root` — for the
/// one `move` test that needs `deps.workspace_index` to actually know
/// about the fixture's documents (everything else uses
/// `EmbeddingDeps::empty()`'s blank one, since `move`'s reference
/// rewrite is a no-op — empty `updated_files` — against a blank
/// index, exercised by the other `move_*` tests below).
pub(crate) fn build_test_workspace_index(repo_root: &Path) -> Arc<WorkspaceIndex> {
    let idx = Arc::new(WorkspaceIndex::new(
        crate::infra::parsers::registry::ParserRegistry::new(),
    ));
    idx.build(repo_root.to_path_buf()).unwrap();
    idx
}
