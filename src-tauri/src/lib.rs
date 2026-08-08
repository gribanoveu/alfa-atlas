mod commands;
mod domain;
mod infra;
mod services;

use domain::settings::{DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, WindowState};
use services::window_settings;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::{LogicalPosition, LogicalSize, Manager, Position, Size, Window, WindowEvent};

use crate::commands::embeddings::{
    BackgroundBacklogSlot, EmbeddingIndexSlot, EmbeddingProviderSlot, EmbeddingSyncGuard,
    IndexStoreSlot, IndexWatcherSlot, PriorityFilesSlot,
};
use crate::commands::llm::LlmProviderSlot;
use crate::infra::parsers::registry::ParserRegistry;
use crate::services::chunk_builder::ChunkIndex;
use crate::services::embedding_model::DownloadState;
use crate::services::repo_index::RepositoryIndex;
use crate::services::spellcheck::SpellcheckEngine;
use crate::services::workspace_index::WorkspaceIndex;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn apply_window_state(window: &tauri::WebviewWindow, state: WindowState) {
    let _ = window.set_min_size(Some(Size::Logical(LogicalSize::new(
        DEFAULT_WINDOW_WIDTH,
        DEFAULT_WINDOW_HEIGHT,
    ))));

    if let Some((x, y)) = state.position() {
        let _ = window.set_position(Position::Logical(LogicalPosition::new(x, y)));
    }

    let _ = window.set_size(Size::Logical(LogicalSize::new(state.width, state.height)));

    if state.maximized {
        let _ = window.maximize();
    }

    let _ = window.show();
}

fn persist_window_state(window: &Window) {
    let Ok(maximized) = window.is_maximized() else {
        return;
    };
    let Ok(scale) = window.scale_factor() else {
        return;
    };

    let mut state = window_settings::load_window_state();
    state.maximized = maximized;

    // While maximized, keep the last restored size/position so unmaximize
    // and the next launch bring the window back to where it was.
    if !maximized {
        let Ok(physical_size) = window.inner_size() else {
            return;
        };
        let logical_size: LogicalSize<f64> = physical_size.to_logical(scale);
        state.width = logical_size.width;
        state.height = logical_size.height;

        if let Ok(physical_pos) = window.outer_position() {
            let logical_pos: LogicalPosition<f64> = physical_pos.to_logical(scale);
            state.x = Some(logical_pos.x);
            state.y = Some(logical_pos.y);
        }
    }

    let _ = window_settings::save_window_state(state);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_http::init());

    // MCP bridge plugin — debug-only, so the Cursor Tauri MCP server can
    // inspect the running app for diagnostics. No effect in production builds.
    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(tauri_plugin_mcp_bridge::init());
    }

    builder
        .setup(|app| {
            let state = window_settings::load_window_state();
            if let Some(window) = app.get_webview_window("main") {
                apply_window_state(&window, state);
            }

            let index = Arc::new(WorkspaceIndex::new(ParserRegistry::new()));
            if let Some(window) = app.get_webview_window("main") {
                index.set_app_handle(window.app_handle().clone());
            }
            app.manage(index);
            app.manage(Arc::new(SpellcheckEngine::load()));
            app.manage(Arc::new(RepositoryIndex::new()));
            app.manage(Arc::new(ChunkIndex::new()));
            app.manage(Arc::new(EmbeddingIndexSlot::new(None)));
            app.manage(Arc::new(IndexStoreSlot::new(None)));
            app.manage(Arc::new(EmbeddingProviderSlot::new(None)));
            app.manage(Arc::new(EmbeddingSyncGuard::new(())));
            app.manage(Arc::new(IndexWatcherSlot::new(None)));
            app.manage(Arc::new(PriorityFilesSlot::new(HashSet::new())));
            app.manage(Arc::new(BackgroundBacklogSlot::new(None)));
            app.manage(Arc::new(DownloadState::default()));
            app.manage(Arc::new(LlmProviderSlot::new(None)));

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::Resized(_)
                | WindowEvent::Moved(_)
                | WindowEvent::CloseRequested { .. } => {
                    persist_window_state(window);
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            exit_app,
            commands::project::probe_open_path,
            commands::project::open_project,
            commands::project::add_gitignore_entry,
            commands::project::open_cached_project,
            commands::project::get_project,
            commands::project::get_saved_repo_root,
            commands::project::clear_project,
            commands::project::list_recent_projects,
            commands::project::remove_recent_project,
            commands::project::get_git_branch,
            commands::git::git_status,
            commands::git::git_stage,
            commands::git::git_unstage,
            commands::git::git_commit,
            commands::git::git_log,
            commands::git::git_pull,
            commands::git::git_conflict_file_content,
            commands::git::git_resolve_conflict,
            commands::git::git_finish_merge,
            commands::git::git_abort_merge,
            commands::git::git_reset_to_remote,
            commands::git::git_sync_status,
            commands::git::git_push,
            commands::git::git_file_diff,
            commands::git::git_commit_files,
            commands::git::git_commit_file_diff,
            commands::git::git_discard_file_changes,
            commands::git::git_apply_diff_content,
            commands::git::git_list_branches,
            commands::git::git_fetch_branches,
            commands::git::git_create_branch,
            commands::git::git_checkout_branch,
            commands::git::git_delete_branch,
            commands::git::git_checkout_remote_branch,
            commands::git::git_get_credentials,
            commands::git::git_save_credentials,
            commands::git::git_get_key_status,
            commands::git::git_generate_key,
            commands::git::git_import_key,
            commands::git::git_clone,
            commands::project::list_docs_tree,
            commands::project::read_project_file,
            commands::project::read_project_file_or_none,
            commands::project::resolve_asset_path,
            commands::project::write_project_file,
            commands::project::create_project_file,
            commands::project::create_project_file_from_template,
            commands::project::create_rest_endpoint_folder,
            commands::project::create_project_dir,
            commands::project::delete_project_file,
            commands::project::delete_project_dir,
            commands::project::rename_project_file,
            commands::project::rename_project_dir,
            commands::project::copy_project_file,
            commands::project::copy_project_dir,
            commands::project::check_path_exists,
            commands::openapi::detect_specs_repo,
            commands::openapi::load_openapi_bundle,
            commands::layout::get_project_layout,
            commands::layout::save_project_layout,
            commands::workspace::get_workspace_state,
            commands::workspace::save_workspace_state,
            commands::prefs::get_general_prefs,
            commands::prefs::set_general_prefs,
            commands::prefs::get_settings_paths,
            commands::onboarding::get_onboarding_state,
            commands::onboarding::mark_onboarding_completed,
            commands::workspace_index::build_index,
            commands::workspace_index::clear_index,
            commands::workspace_index::index_is_open,
            commands::workspace_index::get_document,
            commands::workspace_index::get_documents,
            commands::workspace_index::find_document,
            commands::workspace_index::find_anchor,
            commands::workspace_index::find_anchors,
            commands::workspace_index::find_includes,
            commands::workspace_index::find_references,
            commands::workspace_index::find_attribute,
            commands::workspace_index::get_attributes,
            commands::workspace_index::find_image,
            commands::workspace_index::get_diagnostics,
            commands::workspace_index::get_diagnostics_for,
            commands::asciidoc::submit_asciidoc_facts,
            commands::asciidoc::frontend_ready,
            commands::standards::get_standards_rules,
            commands::standards::get_standards_config,
            commands::standards::set_standards_config,
            commands::standards::check_standards,
            commands::spellcheck::get_dictionaries,
            commands::spellcheck::get_spellcheck_config,
            commands::spellcheck::set_spellcheck_config,
            commands::spellcheck::check_spelling,
            commands::spellcheck::suggest_spelling,
            commands::spellcheck::get_custom_dictionary_words,
            commands::spellcheck::add_custom_dictionary_word,
            commands::spellcheck::remove_custom_dictionary_word,
            commands::ai_tools::ai_execute_tool,
            commands::ai_tools::ai_get_access_mode,
            commands::ai_tools::ai_set_access_mode,
            commands::ai_tools::ai_get_tool_definitions,
            commands::embeddings::embedding_get_config,
            commands::embeddings::embedding_set_config,
            commands::embeddings::embedding_set_remote_api_key,
            commands::embeddings::embedding_has_remote_api_key,
            commands::embeddings::embedding_model_status,
            commands::embeddings::embedding_download_model,
            commands::embeddings::embedding_cancel_model_download,
            commands::embeddings::embedding_sync,
            commands::embeddings::embedding_index_status,
            commands::embeddings::embedding_index_teardown,
            commands::embeddings::embedding_set_priority_files,
            commands::llm::llm_get_settings,
            commands::llm::llm_set_settings,
            commands::llm::llm_list_providers,
            commands::llm::llm_upsert_provider,
            commands::llm::llm_remove_provider,
            commands::llm::llm_set_api_key,
            commands::llm::llm_has_api_key,
            commands::llm::llm_list_models,
            commands::llm::llm_test_connection,
            commands::llm::llm_chat_stream,
            commands::llm::llm_chat_stream_resume,
            commands::chat_history::chat_list,
            commands::chat_history::chat_load_messages,
            commands::chat_history::chat_save,
            commands::chat_history::chat_set_archived,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
