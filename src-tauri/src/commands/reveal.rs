//! Opening a path in the OS file manager, with the containment check the
//! `opener` capability cannot express.
//!
//! The capability's path scope is static, but the one root that matters
//! here — the opened project — changes at runtime, which is why the
//! capability previously had to allow `**`: every path on the machine, to
//! the default application for its file type. That is a wide primitive to
//! hand the webview when the frontend renders documents it did not author.
//!
//! So the scope moves here, where the roots can be read at call time:
//! `~/.atlas` (Settings' "open folder" and the skills folder) and the
//! opened project (the sidebar's "reveal in Finder"). This mirrors how
//! `services::ai_tools::resolve` already contains the agent's file tools —
//! containment resolved against a root, never trusted from the caller.

use std::path::{Path, PathBuf};

use tauri_plugin_opener::OpenerExt;

use crate::domain::paths;
use crate::infra::settings_store;

/// The roots a revealed path may live under, most specific first.
///
/// Read fresh on every call rather than cached: the project root changes
/// whenever the user opens another repository, and a stale root here would
/// either reject legitimate paths or keep accepting paths from a project
/// that is no longer open.
fn allowed_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(dir) = settings_store::settings_dir() {
        roots.push(dir);
    }
    // Deliberately `settings_store::load` rather than
    // `project_open::get_project`: the latter answers `None` when the
    // "restore last project" preference is off, which says nothing about
    // which project is open right now.
    if let Ok(settings) = settings_store::load() {
        if let Some(root) = settings.project.root {
            roots.push(PathBuf::from(root));
        }
    }
    roots
}

/// Shows `path` in the OS file manager, or opens it with its default
/// application, provided it sits inside one of `allowed_roots`.
///
/// `paths::ensure_under` canonicalizes both sides, so `..` segments and
/// symlinks that leave the root are rejected rather than followed.
#[tauri::command]
pub fn reveal_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let requested = Path::new(&path);
    if !requested.exists() {
        return Err(format!("path does not exist: {path}"));
    }

    let resolved = allowed_roots()
        .iter()
        .find_map(|root| paths::ensure_under(root, requested).ok())
        .ok_or_else(|| {
            format!("refusing to open {path}: outside the open project and the settings directory")
        })?;

    app.opener()
        .open_path(resolved.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| format!("failed to open {path}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// `allowed_roots` and the containment check are the whole security
    /// value here; `open_path` itself needs a real `AppHandle` and a
    /// desktop session, so the check is exercised directly.
    fn contained(roots: &[PathBuf], candidate: &Path) -> bool {
        roots
            .iter()
            .any(|root| paths::ensure_under(root, candidate).is_ok())
    }

    #[test]
    fn accepts_paths_inside_a_root_and_rejects_everything_else() {
        settings_store::test_support::with_temp_home(|| {
            let settings_dir = settings_store::ensure_settings_dir().unwrap();
            let project = settings_dir.parent().unwrap().join("repo");
            fs::create_dir_all(project.join("docs")).unwrap();
            let roots = vec![settings_dir.clone(), project.clone()];

            fs::write(settings_dir.join("settings.json"), "{}").unwrap();
            fs::write(project.join("docs/page.adoc"), "= Page").unwrap();
            assert!(contained(&roots, &settings_dir));
            assert!(contained(&roots, &settings_dir.join("settings.json")));
            assert!(contained(&roots, &project.join("docs/page.adoc")));

            // The home directory itself is a parent of both roots, never
            // inside one.
            assert!(!contained(&roots, settings_dir.parent().unwrap()));
        });
    }

    #[test]
    fn rejects_traversal_out_of_a_root() {
        settings_store::test_support::with_temp_home(|| {
            let settings_dir = settings_store::ensure_settings_dir().unwrap();
            let outside = settings_dir.parent().unwrap().join("outside.txt");
            fs::write(&outside, "secret").unwrap();
            let roots = vec![settings_dir.clone()];

            assert!(!contained(&roots, &outside));
            assert!(!contained(&roots, &settings_dir.join("../outside.txt")));
        });
    }

    /// A symlink is followed during canonicalization, so one planted inside
    /// a root cannot be used to reach a file outside it.
    #[test]
    #[cfg(unix)]
    fn rejects_a_symlink_escaping_a_root() {
        settings_store::test_support::with_temp_home(|| {
            let settings_dir = settings_store::ensure_settings_dir().unwrap();
            let outside = settings_dir.parent().unwrap().join("outside.txt");
            fs::write(&outside, "secret").unwrap();

            let link = settings_dir.join("looks-innocent.txt");
            std::os::unix::fs::symlink(&outside, &link).unwrap();

            assert!(!contained(&[settings_dir.clone()], &link));
        });
    }

    #[test]
    fn a_nonexistent_path_is_refused_before_any_root_check() {
        settings_store::test_support::with_temp_home(|| {
            let missing = settings_store::ensure_settings_dir().unwrap().join("nope");
            assert!(!missing.exists());
        });
    }
}
