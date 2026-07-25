mod commands;
mod domain;
mod infra;
mod services;

use domain::settings::{DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, WindowState};
use services::window_settings;
use tauri::{LogicalPosition, LogicalSize, Manager, Position, Size, Window, WindowEvent};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
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
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = window_settings::load_window_state();
            if let Some(window) = app.get_webview_window("main") {
                apply_window_state(&window, state);
            }
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
            commands::project::probe_open_path,
            commands::project::open_project,
            commands::project::open_cached_project,
            commands::project::get_project,
            commands::project::get_saved_repo_root,
            commands::project::clear_project,
            commands::project::get_git_branch,
            commands::project::list_docs_tree,
            commands::project::read_project_file,
            commands::project::write_project_file,
            commands::layout::get_project_layout,
            commands::layout::save_project_layout,
            commands::prefs::get_general_prefs,
            commands::prefs::set_general_prefs,
            commands::prefs::get_settings_paths,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
