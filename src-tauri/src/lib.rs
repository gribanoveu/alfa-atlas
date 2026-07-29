mod commands;
mod domain;
mod infra;
mod services;

use domain::settings::{DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, WindowState};
use services::window_settings;
use std::sync::Arc;
use tauri::{LogicalPosition, LogicalSize, Manager, Position, Size, Window, WindowEvent};

use crate::infra::parsers::registry::ParserRegistry;
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
        .plugin(tauri_plugin_dialog::init());

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
            commands::git::git_reset_to_remote,
            commands::git::git_sync_status,
            commands::git::git_push,
            commands::git::git_file_diff,
            commands::git::git_discard_file_changes,
            commands::git::git_list_branches,
            commands::git::git_create_branch,
            commands::git::git_checkout_branch,
            commands::git::git_get_credentials,
            commands::git::git_save_credentials,
            commands::git::git_get_key_status,
            commands::git::git_generate_key,
            commands::git::git_import_key,
            commands::git::git_clone,
            commands::project::list_docs_tree,
            commands::project::read_project_file,
            commands::project::resolve_asset_path,
            commands::project::write_project_file,
            commands::project::create_project_file,
            commands::project::create_project_dir,
            commands::project::delete_project_file,
            commands::project::delete_project_dir,
            commands::project::rename_project_file,
            commands::project::rename_project_dir,
            commands::project::check_path_exists,
            commands::layout::get_project_layout,
            commands::layout::save_project_layout,
            commands::workspace::get_workspace_state,
            commands::workspace::save_workspace_state,
            commands::prefs::get_general_prefs,
            commands::prefs::set_general_prefs,
            commands::prefs::get_settings_paths,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
