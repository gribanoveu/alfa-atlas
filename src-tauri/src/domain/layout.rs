use serde::{Deserialize, Serialize};

pub const DEFAULT_SIDEBAR_WIDTH: f64 = 220.0;
pub const DEFAULT_RIGHT_WIDTH: f64 = 340.0;
pub const DEFAULT_BOTTOM_HEIGHT: f64 = 220.0;
pub const DEFAULT_EXTERNAL_HEIGHT: f64 = 160.0;

pub const MIN_SIDEBAR_WIDTH: f64 = 160.0;
pub const MAX_SIDEBAR_WIDTH: f64 = 480.0;
pub const MIN_RIGHT_WIDTH: f64 = 200.0;
pub const MAX_RIGHT_WIDTH: f64 = 560.0;
pub const MIN_BOTTOM_HEIGHT: f64 = 120.0;
pub const MAX_BOTTOM_HEIGHT: f64 = 480.0;
pub const MIN_EXTERNAL_HEIGHT: f64 = 80.0;
pub const MAX_EXTERNAL_HEIGHT: f64 = 400.0;

fn default_external_height() -> f64 {
    DEFAULT_EXTERNAL_HEIGHT
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelLayout {
    pub sidebar_width: f64,
    pub right_width: f64,
    pub bottom_height: f64,
    #[serde(default = "default_external_height")]
    pub external_height: f64,
}

impl PanelLayout {
    pub const fn default_layout() -> Self {
        Self {
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            right_width: DEFAULT_RIGHT_WIDTH,
            bottom_height: DEFAULT_BOTTOM_HEIGHT,
            external_height: DEFAULT_EXTERNAL_HEIGHT,
        }
    }

    pub fn clamped(self) -> Self {
        Self {
            sidebar_width: self.sidebar_width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH),
            right_width: self.right_width.clamp(MIN_RIGHT_WIDTH, MAX_RIGHT_WIDTH),
            bottom_height: self.bottom_height.clamp(MIN_BOTTOM_HEIGHT, MAX_BOTTOM_HEIGHT),
            external_height: self
                .external_height
                .clamp(MIN_EXTERNAL_HEIGHT, MAX_EXTERNAL_HEIGHT),
        }
    }
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self::default_layout()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_out_of_range() {
        let layout = PanelLayout {
            sidebar_width: 10.0,
            right_width: 900.0,
            bottom_height: 50.0,
            external_height: 10.0,
        }
        .clamped();
        assert_eq!(layout.sidebar_width, MIN_SIDEBAR_WIDTH);
        assert_eq!(layout.right_width, MAX_RIGHT_WIDTH);
        assert_eq!(layout.bottom_height, MIN_BOTTOM_HEIGHT);
        assert_eq!(layout.external_height, MIN_EXTERNAL_HEIGHT);
    }

    #[test]
    fn roundtrips_camel_case_json() {
        let json = r#"{"sidebarWidth":240.0,"rightWidth":300.0,"bottomHeight":180.0,"externalHeight":140.0}"#;
        let layout: PanelLayout = serde_json::from_str(json).unwrap();
        assert_eq!(layout.sidebar_width, 240.0);
        assert_eq!(layout.right_width, 300.0);
        assert_eq!(layout.bottom_height, 180.0);
        assert_eq!(layout.external_height, 140.0);
        let out = serde_json::to_string(&layout).unwrap();
        assert!(out.contains("sidebarWidth"));
    }

    #[test]
    fn defaults_missing_external_height() {
        let json = r#"{"sidebarWidth":240.0,"rightWidth":300.0,"bottomHeight":180.0}"#;
        let layout: PanelLayout = serde_json::from_str(json).unwrap();
        assert_eq!(layout.external_height, DEFAULT_EXTERNAL_HEIGHT);
    }
}
