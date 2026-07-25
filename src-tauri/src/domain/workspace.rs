use serde::{Deserialize, Serialize};

fn default_expanded_dirs() -> Vec<String> {
    vec![".".to_string()]
}

fn default_true() -> bool {
    true
}

fn default_right_tool() -> Option<String> {
    Some("assistant".to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceState {
    #[serde(default)]
    pub open_tabs: Vec<String>,
    #[serde(default)]
    pub active_tab: Option<String>,
    #[serde(default = "default_expanded_dirs")]
    pub expanded_dirs: Vec<String>,
    #[serde(default = "default_true")]
    pub sidebar_open: bool,
    #[serde(default = "default_right_tool")]
    pub right_tool: Option<String>,
    #[serde(default)]
    pub bottom_tool: Option<String>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            open_tabs: Vec::new(),
            active_tab: None,
            expanded_dirs: default_expanded_dirs(),
            sidebar_open: true,
            right_tool: default_right_tool(),
            bottom_tool: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_camel_case_json() {
        let json = r#"{"openTabs":["a.adoc"],"activeTab":"a.adoc","expandedDirs":[".", "docs"],"sidebarOpen":false,"rightTool":"git","bottomTool":"suggestions"}"#;
        let state: WorkspaceState = serde_json::from_str(json).unwrap();
        assert_eq!(state.open_tabs, vec!["a.adoc"]);
        assert_eq!(state.active_tab.as_deref(), Some("a.adoc"));
        assert_eq!(state.expanded_dirs, vec![".", "docs"]);
        assert!(!state.sidebar_open);
        assert_eq!(state.right_tool.as_deref(), Some("git"));
        assert_eq!(state.bottom_tool.as_deref(), Some("suggestions"));
    }

    #[test]
    fn defaults_missing_fields() {
        let state: WorkspaceState = serde_json::from_str("{}").unwrap();
        assert!(state.open_tabs.is_empty());
        assert_eq!(state.active_tab, None);
        assert_eq!(state.expanded_dirs, vec![".".to_string()]);
        assert!(state.sidebar_open);
        assert_eq!(state.right_tool.as_deref(), Some("assistant"));
        assert_eq!(state.bottom_tool, None);
    }
}
