//! The two persisted axes of AI access: which root the tools resolve
//! against (`AiAccessMode`) and which tools the project permits at all (the
//! allowlist, plus the separate auto-approve set). Both live in
//! `ProjectConfig`; this is the only module that reads or writes them, and
//! the only one that builds a `ToolScope` out of them.

use std::collections::HashSet;
use std::path::Path;

use crate::domain::ai_access::{AiAccessMode, ToolName, default_allowed_tools, no_project_tools};
use crate::domain::ai_tools::{ToolScope, is_valid_dep_name};
use crate::domain::paths;
use crate::domain::project_config::{ExtraRoot, ProjectConfig, ProjectError};
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
///
/// Filtered through `ToolName::auto_approvable`, so a grant that is no
/// longer allowed to exist stops taking effect the moment this ships —
/// projects that ticked "Разрешать всегда" for a consent tool while that
/// was still possible keep the stale row in their config, and it is simply
/// ignored here rather than needing a config migration.
pub fn auto_approved_tools() -> Result<HashSet<ToolName>, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    Ok(config
        .ai_auto_approved_tools
        .unwrap_or_default()
        .into_iter()
        .filter(|tool| tool.auto_approvable())
        .collect())
}

/// Persists (or revokes) one tool's "always allow" status for the currently
/// open project — the backend counterpart to the approval card's "Разрешать
/// всегда" button. Only ever changes whether a *future* call still pauses
/// for confirmation; it never widens `ai_allowed_tools`, so a tool the
/// project has otherwise disallowed stays disallowed regardless.
///
/// Granting is refused for a tool `ToolName::auto_approvable` says can
/// never be auto-approved (the consent tools); revoking one is always
/// allowed, so a stale grant can still be cleared. The frontend doesn't
/// offer those toggles at all — this is the backstop, since the persisted
/// config outlives any one UI.
pub fn set_tool_auto_approved(tool: ToolName, auto_approved: bool) -> Result<(), ProjectError> {
    if auto_approved && !tool.auto_approvable() {
        return Err(ProjectError::Message(format!(
            "tool {tool:?} can never be auto-approved — it is a consent gate, not a convenience"
        )));
    }
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

/// The project's configured external read-only roots, in the order they
/// were added. Empty (not an error) for a project that has never added one.
///
/// Reports what is *persisted*, which is deliberately not the same as what
/// the assistant currently sees: `ToolScope::with_extra_roots` additionally
/// drops roots whose directory has since disappeared, and every root while
/// the project is in Docs-only mode. The Settings list has to show a stale
/// row so it can be removed — hiding it would leave the user unable to clean
/// up the entry that is confusing them.
pub fn extra_roots() -> Result<Vec<ExtraRoot>, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    Ok(config.ai_extra_roots.unwrap_or_default())
}

/// External roots this project plainly has but has not added — today, a
/// `node_modules` sitting beside a `package.json` at the repository root.
///
/// Suggested, never added on its own: widening what the assistant may read
/// is the user's decision, and a root that appeared without being asked for
/// is exactly the kind of surprise that makes people distrust the whole
/// feature. Already-configured roots drop out, by name or by path, so a
/// suggestion the user has acted on stops being offered.
pub fn suggest_extra_roots() -> Result<Vec<ExtraRoot>, ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let repo = Path::new(&opened.root);

    let mut found = Vec::new();
    let node_modules = repo.join("node_modules");
    if repo.join("package.json").is_file() && node_modules.is_dir() {
        if let Ok(canonical) = paths::canonicalize_plain(&node_modules) {
            found.push(ExtraRoot {
                name: "node_modules".to_string(),
                path: canonical.to_string_lossy().into_owned(),
            });
        }
    }

    let configured = extra_roots()?;
    found.retain(|s| {
        !configured
            .iter()
            .any(|c| c.name == s.name || c.path == s.path)
    });
    Ok(found)
}

/// Adds one external read-only root to the open project.
///
/// Every failure here is a refusal with a reason rather than a silent drop:
/// the user is adding this root right now, and a row that quietly fails to
/// appear reads as a broken feature. `name` must be usable as the single
/// `@deps` path segment that addresses the root, `path` must be a directory
/// that exists, and the name must be free — an add that replaced an existing
/// root would silently redirect every `@deps/{name}/…` path the assistant
/// has already been told about.
pub fn add_extra_root(name: String, path: String) -> Result<(), ProjectError> {
    if !is_valid_dep_name(&name) {
        return Err(ProjectError::Message(format!(
            "недопустимое имя источника «{name}»: нужно одно имя без «/», не начинающееся с точки"
        )));
    }
    let dir = Path::new(&path);
    if !dir.is_dir() {
        return Err(ProjectError::Message(format!(
            "не найдена папка: {path}"
        )));
    }
    let canonical = paths::canonicalize_plain(dir)
        .map_err(|e| ProjectError::Message(format!("не удалось открыть {path}: {e}")))?;

    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let mut config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    let mut roots = config.ai_extra_roots.unwrap_or_default();
    if roots.iter().any(|r| r.name == name) {
        return Err(ProjectError::Message(format!(
            "источник с именем «{name}» уже добавлен"
        )));
    }
    roots.push(ExtraRoot {
        name,
        path: canonical.to_string_lossy().into_owned(),
    });
    config.ai_extra_roots = Some(roots);
    project_store::save(&opened.root, &config)
}

/// Removes one external root by name. Removing what is not there succeeds:
/// the caller wanted it gone, and it is.
pub fn remove_extra_root(name: &str) -> Result<(), ProjectError> {
    let opened = project_open::get_project()?
        .ok_or_else(|| ProjectError::Message("no project is open".to_string()))?;
    let mut config = project_store::load(&opened.root)?
        .unwrap_or_else(|| ProjectConfig::new(opened.docs_root.clone()));
    let mut roots = config.ai_extra_roots.unwrap_or_default();
    roots.retain(|r| r.name != name);
    config.ai_extra_roots = Some(roots);
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
        .with_extra_roots(config.ai_extra_roots.clone().unwrap_or_default())
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
    open_project_scope()?.ok_or_else(|| ProjectError::Message("no project is open".to_string()))
}

/// `current_scope()`, but "no project is open" resolves to a scope with no
/// roots and only `no_project_tools()` allowed, instead of an error — that's
/// what lets the assistant chat stay usable with no project (general
/// questions, a skill-guided Jira ticket draft, a diagram). Every repository
/// tool is absent from the allowlist, so none is advertised to the model
/// and, since `execute_tool` re-checks the same set, none is executable even
/// if one were hallucinated. Every *other* failure (an unreadable or corrupt
/// `project.json`) still errors: silently degrading a real project to a
/// near-tool-less assistant would hide it.
pub fn current_scope_or_empty() -> Result<ToolScope, ProjectError> {
    Ok(open_project_scope()?.unwrap_or_else(|| {
        ToolScope::new(
            Path::new(""),
            Path::new(""),
            AiAccessMode::DocsOnly,
            no_project_tools(),
        )
    }))
}

fn open_project_scope() -> Result<Option<ToolScope>, ProjectError> {
    let Some(opened) = project_open::get_project()? else {
        return Ok(None);
    };
    let config = load_project_config_migrated(&opened.root, &opened.docs_root)?;
    Ok(Some(scope_for_config(
        Path::new(&opened.root),
        Path::new(&opened.docs_root),
        &config,
    )))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::domain::ai_tools::ToolError;
    use crate::domain::project_config::ExtraRoot;

    use super::super::testing::*;
    use super::*;

    /// The chat stays available with no project open, but only the
    /// project-free tools are reachable — `current_scope` still errors for
    /// `ai_execute_tool`, while `current_scope_or_empty` degrades to a scope
    /// that advertises and permits exactly `no_project_tools()`.
    #[test]
    fn current_scope_or_empty_offers_only_project_free_tools() {
        crate::infra::settings_store::test_support::with_temp_home(|| {
            assert!(current_scope().is_err());

            let scope = current_scope_or_empty().unwrap();
            assert!(scope.allows(ToolName::Skill));
            assert!(!scope.allows(ToolName::ReadFile));

            let advertised: Vec<String> = super::super::llm_tool_definitions(
                &scope,
                crate::domain::conversation_mode::ConversationMode::Agent,
            )
            .into_iter()
            .map(|d| d.name)
            .collect();
            assert!(advertised.contains(&"skill".to_string()));
            assert!(advertised.contains(&"visualize".to_string()));
            assert!(!advertised.iter().any(|n| n == "readFile" || n == "writeFile"));
        });
    }

    /// The production path a real project takes: `project.json` carries the
    /// external roots, and the scope the executor runs with has them
    /// attached. Every other test builds a scope directly, so without this
    /// the config could stop being read and nothing would notice.
    /// Opens a real project under a temp home so the config-writing paths
    /// (`add_extra_root`/`remove_extra_root`, which all start by resolving
    /// the open project) can be exercised end to end.
    fn with_open_fixture_project<T>(f: impl FnOnce(std::path::PathBuf) -> T) -> T {
        crate::infra::settings_store::test_support::with_temp_home(|| {
            let (repo, docs) = fixture_repo();
            crate::services::project_open::open_project(
                repo.to_str().unwrap(),
                docs.to_str().unwrap(),
            )
            .unwrap();
            let out = f(repo.clone());
            fs::remove_dir_all(&repo).ok();
            out
        })
    }

    #[test]
    fn an_added_root_persists_and_reaches_the_scope_the_executor_runs_with() {
        with_open_fixture_project(|_repo| {
            let dep = fixture_dep_root();
            add_extra_root("acme".to_string(), dep.to_string_lossy().into_owned()).unwrap();
            set_access_mode(AiAccessMode::FullRepo).unwrap();

            let persisted = extra_roots().unwrap();
            assert_eq!(persisted.len(), 1);
            assert_eq!(persisted[0].name, "acme");

            let scope = current_scope().unwrap();
            assert!(read(&scope, "@deps/acme/lib/Client.java").is_ok());

            fs::remove_dir_all(&dep).ok();
        });
    }

    /// The name becomes a path segment, so it is validated where it enters —
    /// and refused out loud, not dropped.
    #[test]
    fn node_modules_beside_a_package_json_is_suggested_once() {
        with_open_fixture_project(|repo| {
            assert!(suggest_extra_roots().unwrap().is_empty(), "nothing to suggest yet");

            fs::write(repo.join("package.json"), "{}\n").unwrap();
            fs::create_dir_all(repo.join("node_modules/lodash")).unwrap();

            let suggested = suggest_extra_roots().unwrap();
            assert_eq!(suggested.len(), 1);
            assert_eq!(suggested[0].name, "node_modules");

            // Once acted on, it stops being offered.
            add_extra_root(suggested[0].name.clone(), suggested[0].path.clone()).unwrap();
            assert!(suggest_extra_roots().unwrap().is_empty());
        });
    }

    /// A `node_modules` with no manifest beside it is some other project's
    /// leftovers, not this one's dependencies.
    #[test]
    fn node_modules_without_a_package_json_is_not_suggested() {
        with_open_fixture_project(|repo| {
            fs::create_dir_all(repo.join("node_modules/lodash")).unwrap();

            assert!(suggest_extra_roots().unwrap().is_empty());
        });
    }

    #[test]
    fn add_extra_root_refuses_a_name_that_is_not_a_single_segment() {
        with_open_fixture_project(|_repo| {
            let dep = fixture_dep_root();
            for bad in ["a/b", "..", "", ".hidden"] {
                let err = add_extra_root(bad.to_string(), dep.to_string_lossy().into_owned())
                    .unwrap_err();
                assert!(format!("{err}").contains("имя источника"), "{bad}: {err}");
            }
            assert!(extra_roots().unwrap().is_empty());

            fs::remove_dir_all(&dep).ok();
        });
    }

    #[test]
    fn add_extra_root_refuses_a_path_that_is_not_a_directory() {
        with_open_fixture_project(|repo| {
            let err = add_extra_root(
                "acme".to_string(),
                repo.join("nothing-here").to_string_lossy().into_owned(),
            )
            .unwrap_err();

            assert!(format!("{err}").contains("не найдена папка"), "{err}");
        });
    }

    /// Re-adding a name would silently redirect every `@deps/{name}/…` path
    /// the assistant has already been given.
    #[test]
    fn add_extra_root_refuses_a_name_already_in_use() {
        with_open_fixture_project(|_repo| {
            let first = fixture_dep_root();
            let second = fixture_dep_root();
            add_extra_root("acme".to_string(), first.to_string_lossy().into_owned()).unwrap();

            let err = add_extra_root("acme".to_string(), second.to_string_lossy().into_owned())
                .unwrap_err();

            assert!(format!("{err}").contains("уже добавлен"), "{err}");
            let roots = extra_roots().unwrap();
            assert_eq!(roots.len(), 1);
            assert_eq!(roots[0].path, first.to_string_lossy());

            fs::remove_dir_all(&first).ok();
            fs::remove_dir_all(&second).ok();
        });
    }

    #[test]
    fn removing_a_root_that_is_not_there_still_succeeds() {
        with_open_fixture_project(|_repo| {
            let dep = fixture_dep_root();
            add_extra_root("acme".to_string(), dep.to_string_lossy().into_owned()).unwrap();

            remove_extra_root("acme").unwrap();
            remove_extra_root("acme").unwrap();

            assert!(extra_roots().unwrap().is_empty());

            fs::remove_dir_all(&dep).ok();
        });
    }

    #[test]
    fn scope_for_config_attaches_the_configured_external_roots() {
        let (repo, docs) = fixture_repo();
        let dep = fixture_dep_root();
        let mut config = ProjectConfig::new(".");
        config.ai_access_mode = AiAccessMode::FullRepo;
        config.ai_extra_roots = Some(vec![
            ExtraRoot {
                name: "acme".to_string(),
                path: dep.to_string_lossy().into_owned(),
            },
            // Dropped: the path does not exist. A stale entry costs one
            // unresolvable name, not the scope.
            ExtraRoot {
                name: "gone".to_string(),
                path: dep.join("removed-long-ago").to_string_lossy().into_owned(),
            },
        ]);

        let scope = scope_for_config(&repo, &docs, &config);

        let names: Vec<&str> = scope.extra_roots().iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["acme"]);
        assert!(read(&scope, "@deps/acme/lib/Client.java").is_ok());

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&dep).ok();
    }

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
