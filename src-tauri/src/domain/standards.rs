//! Domain types for the API-documentation standards checker.
//!
//! Rules themselves (and their check logic) live in code under
//! `services/standards_rules.rs` — these types are the serializable data
//! shapes shared between the rule registry, the orchestration service, and
//! the frontend.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Static metadata for one standard rule (e.g. "К.1.1"). The rule's actual
/// check logic is a separate `fn` paired with this in the rule registry —
/// this type only carries what the frontend needs to render a toggle list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleDef {
    pub id: &'static str,
    pub title: &'static str,
    pub weight: u32,
    pub default_enabled: bool,
    /// Rules that would need network access (link/endpoint checks) are
    /// registered but not runnable yet; the UI disables their toggle.
    pub requires_network: bool,
}

/// Outcome of running one rule's check function against a method folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleOutcome {
    pub passed: bool,
    pub message: String,
}

/// One rule's result for one method folder, ready to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub rule_id: String,
    pub title: String,
    pub passed: bool,
    pub weight: u32,
    pub message: String,
}

/// Result of checking one `methodName` folder against all enabled rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderReport {
    /// Path relative to the docs root (e.g. `getUserInfo`).
    pub folder: String,
    pub method_name: String,
    pub score: u32,
    pub max_score: u32,
    /// `score / max_score > 0.8`, matching the standard's 80% threshold.
    pub passed: bool,
    pub findings: Vec<Finding>,
}

/// Result of checking every method folder under the docs root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StandardsReport {
    pub folders: Vec<FolderReport>,
    /// True only if at least one folder was found and every folder passed.
    pub overall_passed: bool,
    pub checked_at: u64,
}

/// Persisted enable/disable overrides, keyed by `RuleDef::id`. A rule absent
/// from the map falls back to `RuleDef::default_enabled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StandardsRuleConfig {
    pub rules: HashMap<String, bool>,
}

impl StandardsRuleConfig {
    pub fn is_enabled(&self, def: &RuleDef) -> bool {
        self.rules.get(def.id).copied().unwrap_or(def.default_enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_falls_back_to_default_enabled() {
        let def = RuleDef {
            id: "K.1.1",
            title: "test",
            weight: 20,
            default_enabled: true,
            requires_network: false,
        };
        let config = StandardsRuleConfig::default();
        assert!(config.is_enabled(&def));

        let mut config = StandardsRuleConfig::default();
        config.rules.insert("K.1.1".to_string(), false);
        assert!(!config.is_enabled(&def));
    }
}
