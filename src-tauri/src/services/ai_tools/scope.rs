//! The two persisted axes of AI access: which root the tools resolve
//! against (`AiAccessMode`) and which tools the project permits at all (the
//! allowlist, plus the separate auto-approve set). Both live in
//! `ProjectConfig`; this is the only module that reads or writes them, and
//! the only one that builds a `ToolScope` out of them.

use std::collections::HashSet;
use std::path::Path;

use crate::domain::ai_access::{AiAccessMode, ToolName, default_allowed_tools};
use crate::domain::ai_tools::ToolScope;
use crate::domain::project_config::{ProjectConfig, ProjectError};
use crate::infra::project_store;
use crate::services::project_open;

/// Persists a new `AiAccessMode` for the currently open project — shared by
/// the manual `commands::ai_tools::ai_set_access_mode` toggle and the
/// `RequestFullRepoAccess` tool, so a mode change behaves identically
/// regardless of which path triggered it. Preserves any existing
/// `ai_allowed_tools` override rather than resetting it.
pub fn set_access_mode(mode: AiAccessMode) -> Result<(), ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let mut config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    config.ai_access_mode = mode;
    project_store::save(&opened.root, &config)
}

/// Tool names the currently open project has persisted as "don't ask for
/// confirmation again" (`ProjectConfig::ai_auto_approved_tools`) — read by
/// the frontend once per chat panel mount to seed its in-memory trusted-tool
/// set, so a choice made in one chat carries into every later chat on the
/// same repo. Empty when the project has never customized this (matches the
/// `None` default), not an error.
pub fn auto_approved_tools() -> Result<HashSet<ToolName>, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    Ok(config.ai_auto_approved_tools.unwrap_or_default().into_iter().collect())
}

/// Persists (or revokes) one tool's "always allow" status for the currently
/// open project — the backend counterpart to the approval card's "Разрешать
/// всегда" button. Only ever changes whether a *future* call still pauses
/// for confirmation; it never widens `ai_allowed_tools`, so a tool the
/// project has otherwise disallowed stays disallowed regardless.
pub fn set_tool_auto_approved(tool: ToolName, auto_approved: bool) -> Result<(), ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let mut config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    let mut set: HashSet<ToolName> = config.ai_auto_approved_tools.unwrap_or_default().into_iter().collect();
    if auto_approved {
        set.insert(tool);
    } else {
        set.remove(&tool);
    }
    config.ai_auto_approved_tools = Some(set.into_iter().collect());
    project_store::save(&opened.root, &config)
}

/// Tool names the currently open project's `ai_allowed_tools` currently
/// resolves to — the customized set if one was ever saved, else `mode`'s
/// default (mirrors `scope_for_config`'s own resolution exactly, so what
/// this reports is always what `execute_tool` actually enforces).
pub fn allowed_tools() -> Result<HashSet<ToolName>, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let config = load_project_config_migrated(&opened.root, &opened.docs_root)?;
    Ok(config
        .ai_allowed_tools
        .clone()
        .unwrap_or_else(|| default_allowed_tools(config.ai_access_mode).into_iter().collect())
        .into_iter()
        .collect())
}

/// Persists (or revokes) one tool's membership in `ai_allowed_tools` for the
/// currently open project — the backend counterpart to a new Settings UI
/// checkbox. Seeds the customized set from the current default (rather than
/// starting from empty) the first time any tool is toggled, so unchecking
/// one tool doesn't silently disallow every other tool too.
pub fn set_tool_allowed(tool: ToolName, allowed: bool) -> Result<(), ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let mut config = load_project_config_migrated(&opened.root, &opened.docs_root)?;
    let mut set: HashSet<ToolName> = config
        .ai_allowed_tools
        .clone()
        .map(|v| v.into_iter().collect())
        .unwrap_or_else(|| default_allowed_tools(config.ai_access_mode));
    if allowed {
        set.insert(tool);
    } else {
        set.remove(&tool);
    }
    config.ai_allowed_tools = Some(set.into_iter().collect());
    project_store::save(&opened.root, &config)
}

/// `ToolName` variants introduced by the plan-mode feature. A project whose
/// `ai_allowed_tools` was customized (Settings → Permissions) before these
/// variants existed cannot have intentionally revoked them — they weren't
/// yet options to revoke. See `migrate_plan_tools_into_allowlist`.
const PLAN_TOOLS_MIGRATION: [ToolName; 4] = [
    ToolName::CreatePlan,
    ToolName::UpdatePlan,
    ToolName::ReadPlan,
    ToolName::UpdatePlanTodo,
];

/// Same backfill reason as `PLAN_TOOLS_MIGRATION` for the Agent Skills router.
const SKILL_TOOL_MIGRATION: [ToolName; 1] = [ToolName::Skill];

/// Backfills `config.ai_allowed_tools` with any `PLAN_TOOLS_MIGRATION` tool
/// missing from an already-customized list, so a project saved before this
/// feature shipped doesn't permanently lose access to it — `ToolName`
/// variants added later never automatically widen a customized allowlist
/// (see `default_allowed_tools`'s doc comment), so without this a
/// customized project would need the user to manually re-enable each new
/// tool in Settings. No-op when `ai_allowed_tools` is `None` — an
/// uncustomized project already resolves through `default_allowed_tools`,
/// which includes these. Returns whether anything changed, so the caller
/// knows whether to persist.
fn migrate_plan_tools_into_allowlist(config: &mut ProjectConfig) -> bool {
    let Some(list) = config.ai_allowed_tools.as_mut() else {
        return false;
    };
    let mut changed = false;
    for tool in PLAN_TOOLS_MIGRATION.iter().chain(SKILL_TOOL_MIGRATION.iter()) {
        if !list.contains(tool) {
            list.push(*tool);
            changed = true;
        }
    }
    changed
}

/// Shared "load this project's config, catching it up on any pending
/// allowlist migration" used by every call site that resolves
/// `ai_allowed_tools` (`allowed_tools`, `set_tool_allowed`, `current_scope`)
/// — replaces their previous direct
/// `project_store::load(...).unwrap_or_else(...)` so a project's allowlist
/// only needs to catch up once, on whichever of the three runs first.
/// Persists immediately when migration changed anything (mirrors
/// `infra::chat_store`'s ALTER-on-open precedent, just at the project.json
/// layer).
fn load_project_config_migrated(root: &str, docs_root_fallback: &str) -> Result<ProjectConfig, ProjectError> {
    let mut config = project_store::load(root)?.unwrap_or_else(|| ProjectConfig::new(docs_root_fallback));
    if migrate_plan_tools_into_allowlist(&mut config) {
        project_store::save(root, &config)?;
    }
    Ok(config)
}

/// Resolves a `ToolScope` from a project's persisted config — the one place
/// that turns "user hasn't customized anything" into `mode`'s default
/// allowlist, and a customized list into the authoritative one.
pub fn scope_for_config(repo_root: &Path, docs_root: &Path, config: &ProjectConfig) -> ToolScope {
    let allowed: HashSet<ToolName> = config
        .ai_allowed_tools
        .clone()
        .map(|v| v.into_iter().collect())
        .unwrap_or_else(|| default_allowed_tools(config.ai_access_mode));
    ToolScope::new(repo_root, docs_root, config.ai_access_mode, allowed)
}

/// Resolves a `ToolScope` for whichever project is currently open, without
/// the caller (the IPC command) supplying any path — this is what lets the
/// frontend call `ai_execute_tool` knowing nothing about `docsRoot`/
/// `repoRoot`/the access mode. Reuses the same backend-authoritative source
/// `commands::project::get_project` already uses at startup restore;
/// `project_open::get_project()` alone doesn't expose `ai_access_mode`/
/// `ai_allowed_tools` (it discards the rest of `ProjectConfig`), so those
/// are loaded separately here.
pub fn current_scope() -> Result<ToolScope, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let config = load_project_config_migrated(&opened.root, &opened.docs_root)?;
    Ok(scope_for_config(
        Path::new(&opened.root),
        Path::new(&opened.docs_root),
        &config,
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::domain::ai_tools::ToolError;

    use super::super::testing::*;
    use super::*;

    #[test]
    fn scope_for_config_defaults_to_both_tools_when_unset() {
        let (repo, docs) = fixture_repo();
        let config = ProjectConfig::new(".");

        let scope = scope_for_config(&repo, &docs, &config);
        assert!(read(&scope, "intro.adoc").is_ok());
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn scope_for_config_honors_a_customized_allowlist() {
        let (repo, docs) = fixture_repo();
        let mut config = ProjectConfig::new(".");
        config.ai_allowed_tools = Some(vec![ToolName::ListFiles]);

        let scope = scope_for_config(&repo, &docs, &config);
        assert!(matches!(
            read(&scope, "intro.adoc").unwrap_err(),
            ToolError::NotAllowed(ToolName::ReadFile)
        ));
        assert!(list(&scope, None).is_ok());

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn migrate_plan_tools_into_allowlist_backfills_only_missing_plan_tools() {
        let mut config = ProjectConfig::new(".");
        config.ai_allowed_tools = Some(vec![ToolName::ListFiles, ToolName::ReadPlan]);

        let changed = migrate_plan_tools_into_allowlist(&mut config);

        assert!(changed);
        let list = config.ai_allowed_tools.unwrap();
        assert!(list.contains(&ToolName::CreatePlan));
        assert!(list.contains(&ToolName::UpdatePlan));
        assert_eq!(list.iter().filter(|t| **t == ToolName::ReadPlan).count(), 1);
        assert!(list.contains(&ToolName::UpdatePlanTodo));
        assert!(list.contains(&ToolName::Skill));
        assert!(!list.contains(&ToolName::WriteFile));
    }

    #[test]
    fn migrate_plan_tools_into_allowlist_is_noop_when_unset() {
        let mut config = ProjectConfig::new(".");
        assert!(!migrate_plan_tools_into_allowlist(&mut config));
        assert!(config.ai_allowed_tools.is_none());
    }

    #[test]
    fn migrate_plan_tools_into_allowlist_is_idempotent() {
        let mut config = ProjectConfig::new(".");
        config.ai_allowed_tools = Some(vec![ToolName::ListFiles]);
        assert!(migrate_plan_tools_into_allowlist(&mut config));
        assert!(!migrate_plan_tools_into_allowlist(&mut config));
    }
}
