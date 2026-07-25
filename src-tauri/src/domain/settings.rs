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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProjectSettings {
    #[serde(default)]
    pub root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub window: WindowState,
    #[serde(default)]
    pub project: ProjectSettings,
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
    }
}
