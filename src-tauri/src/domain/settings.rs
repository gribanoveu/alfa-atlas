use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default and minimum logical window size used on first launch
/// and as a floor when restoring a saved size.
pub const DEFAULT_WINDOW_WIDTH: f64 = 800.0;
pub const DEFAULT_WINDOW_HEIGHT: f64 = 600.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub maximized: bool,
}

impl WindowState {
    pub const fn default_state() -> Self {
        Self {
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            x: None,
            y: None,
            maximized: false,
        }
    }

    /// Ensures width/height are at least the configured minimum.
    pub fn clamped(self) -> Self {
        Self {
            width: self.width.max(DEFAULT_WINDOW_WIDTH),
            height: self.height.max(DEFAULT_WINDOW_HEIGHT),
            ..self
        }
    }

    pub fn position(self) -> Option<(f64, f64)> {
        match (self.x, self.y) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        }
    }
}

impl Default for WindowState {
    fn default() -> Self {
        Self::default_state()
    }
}

pub const MAX_RECENT_PROJECTS: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProjectSettings {
    #[serde(default)]
    pub root: Option<String>,
    /// MRU absolute paths (index 0 = most recent). Global `~/.docflow/settings.json` only.
    #[serde(default)]
    pub recent: Vec<String>,
}

impl ProjectSettings {
    /// Move `root` to the front of the recent list (dedupe + cap).
    pub fn push_recent(&mut self, root: &str) {
        self.recent.retain(|path| path != root);
        self.recent.insert(0, root.to_string());
        self.recent.truncate(MAX_RECENT_PROJECTS);
    }

    /// If `recent` is empty but `root` is set, seed one entry (legacy settings).
    pub fn seed_recent_from_root(&mut self) {
        if self.recent.is_empty() {
            if let Some(root) = self.root.clone() {
                self.recent.push(root);
            }
        }
    }
}

pub const DEFAULT_AUTOSAVE_DELAY_MS: u64 = 1000;
pub const MIN_AUTOSAVE_DELAY_MS: u64 = 300;
pub const MAX_AUTOSAVE_DELAY_MS: u64 = 10_000;

fn default_true() -> bool {
    true
}

fn default_autosave_delay_ms() -> u64 {
    DEFAULT_AUTOSAVE_DELAY_MS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorLanguage {
    /// Сообщения диагностик на русском.
    #[serde(rename = "ru")]
    Ru,
    /// Сообщения диагностик на английском.
    #[serde(rename = "en")]
    En,
}

impl Default for ErrorLanguage {
    fn default() -> Self {
        Self::Ru
    }
}

fn default_error_language() -> ErrorLanguage {
    ErrorLanguage::Ru
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralPrefs {
    #[serde(default = "default_true")]
    pub restore_last_project: bool,
    #[serde(default = "default_true")]
    pub autosave_enabled: bool,
    #[serde(default = "default_true")]
    pub save_on_tab_switch: bool,
    #[serde(default = "default_autosave_delay_ms")]
    pub autosave_delay_ms: u64,
    #[serde(default = "default_true")]
    pub separate_external_folder: bool,
    /// Язык сообщений диагностик (Проблемы в нижней панели).
    #[serde(default = "default_error_language")]
    pub error_language: ErrorLanguage,
}

impl GeneralPrefs {
    pub fn clamped(self) -> Self {
        Self {
            autosave_delay_ms: self
                .autosave_delay_ms
                .clamp(MIN_AUTOSAVE_DELAY_MS, MAX_AUTOSAVE_DELAY_MS),
            ..self
        }
    }
}

impl Default for GeneralPrefs {
    fn default() -> Self {
        Self {
            restore_last_project: true,
            autosave_enabled: true,
            save_on_tab_switch: true,
            autosave_delay_ms: DEFAULT_AUTOSAVE_DELAY_MS,
            separate_external_folder: true,
            error_language: ErrorLanguage::Ru,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub window: WindowState,
    #[serde(default)]
    pub project: ProjectSettings,
    #[serde(default)]
    pub general: GeneralPrefs,
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("home directory is unavailable")]
    HomeDirUnavailable,
    #[error("failed to create settings directory: {0}")]
    CreateDir(#[source] std::io::Error),
    #[error("failed to read settings: {0}")]
    Read(#[source] std::io::Error),
    #[error("failed to write settings: {0}")]
    Write(#[source] std::io::Error),
    #[error("failed to parse settings: {0}")]
    Parse(#[source] serde_json::Error),
    #[error("failed to serialize settings: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("project path is not a directory: {0}")]
    NotADirectory(String),
    #[error("failed to resolve project path: {0}")]
    Canonicalize(#[source] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_below_minimum() {
        let state = WindowState {
            width: 100.0,
            height: 50.0,
            ..WindowState::default()
        }
        .clamped();
        assert_eq!(state.width, DEFAULT_WINDOW_WIDTH);
        assert_eq!(state.height, DEFAULT_WINDOW_HEIGHT);
    }

    #[test]
    fn keeps_larger_size() {
        let state = WindowState {
            width: 1200.0,
            height: 900.0,
            ..WindowState::default()
        }
        .clamped();
        assert_eq!(state.width, 1200.0);
        assert_eq!(state.height, 900.0);
    }

    #[test]
    fn position_requires_both_axes() {
        let only_x = WindowState {
            x: Some(10.0),
            y: None,
            ..WindowState::default()
        };
        assert_eq!(only_x.position(), None);

        let both = WindowState {
            x: Some(10.0),
            y: Some(20.0),
            ..WindowState::default()
        };
        assert_eq!(both.position(), Some((10.0, 20.0)));
    }

    #[test]
    fn deserializes_legacy_size_only_json() {
        let state: WindowState =
            serde_json::from_str(r#"{"width":1024.0,"height":768.0}"#).unwrap();
        assert_eq!(state.width, 1024.0);
        assert_eq!(state.height, 768.0);
        assert_eq!(state.x, None);
        assert_eq!(state.y, None);
        assert!(!state.maximized);
    }

    #[test]
    fn deserializes_legacy_settings_without_project() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"window":{"width":800.0,"height":600.0}}"#).unwrap();
        assert_eq!(settings.project.root, None);
        assert!(settings.project.recent.is_empty());
        assert!(settings.general.restore_last_project);
        assert!(settings.general.autosave_enabled);
        assert!(settings.general.save_on_tab_switch);
        assert_eq!(
            settings.general.autosave_delay_ms,
            DEFAULT_AUTOSAVE_DELAY_MS
        );
    }

    #[test]
    fn push_recent_dedupes_and_caps() {
        let mut project = ProjectSettings::default();
        for i in 0..12 {
            project.push_recent(&format!("/p/{i}"));
        }
        assert_eq!(project.recent.len(), MAX_RECENT_PROJECTS);
        assert_eq!(project.recent[0], "/p/11");
        project.push_recent("/p/5");
        assert_eq!(project.recent[0], "/p/5");
        assert_eq!(project.recent.iter().filter(|p| *p == "/p/5").count(), 1);
    }

    #[test]
    fn seed_recent_from_root_when_empty() {
        let mut project = ProjectSettings {
            root: Some("/repo".into()),
            recent: vec![],
        };
        project.seed_recent_from_root();
        assert_eq!(project.recent, vec!["/repo".to_string()]);
        project.seed_recent_from_root();
        assert_eq!(project.recent, vec!["/repo".to_string()]);
    }

    #[test]
    fn deserializes_legacy_general_without_autosave_fields() {
        let prefs: GeneralPrefs =
            serde_json::from_str(r#"{"restoreLastProject":false}"#).unwrap();
        assert!(!prefs.restore_last_project);
        assert!(prefs.autosave_enabled);
        assert!(prefs.save_on_tab_switch);
        assert_eq!(prefs.autosave_delay_ms, DEFAULT_AUTOSAVE_DELAY_MS);
        assert!(prefs.separate_external_folder);
        assert_eq!(prefs.error_language, ErrorLanguage::Ru);
    }

    #[test]
    fn clamps_autosave_delay() {
        let prefs = GeneralPrefs {
            autosave_delay_ms: 10,
            ..GeneralPrefs::default()
        }
        .clamped();
        assert_eq!(prefs.autosave_delay_ms, MIN_AUTOSAVE_DELAY_MS);
    }

    #[test]
    fn deserializes_error_language_ru_and_en() {
        let ru: GeneralPrefs = serde_json::from_str(r#"{"errorLanguage":"ru"}"#).unwrap();
        assert_eq!(ru.error_language, ErrorLanguage::Ru);
        let en: GeneralPrefs = serde_json::from_str(r#"{"errorLanguage":"en"}"#).unwrap();
        assert_eq!(en.error_language, ErrorLanguage::En);
    }
}
