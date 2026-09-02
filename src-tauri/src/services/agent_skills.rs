//! Merge bundled + user Agent Skills, apply enable/disable, and serve the
//! `skill` tool ops (`search` / `load` / `read`) plus Settings IPC.

use crate::domain::agent_skills::{
    merge_catalog, search_skills, SkillError, SkillListItem, SkillMeta, SkillSource, SkillsSettings,
};
use crate::domain::ai_tools::{
    SkillFileResult, SkillLoadedResult, SkillSearchHit, SkillSearchResult, ToolResult,
};
use crate::domain::settings::SettingsError;
use crate::infra::{bundled_skills, settings_store, user_skills_store};

const SKILL_MD: &str = "SKILL.md";

pub fn load_skills_settings() -> Result<SkillsSettings, SettingsError> {
    Ok(settings_store::load()?.skills)
}

pub fn save_skills_settings(skills: SkillsSettings) -> Result<(), SettingsError> {
    let mut settings = settings_store::load().unwrap_or_default();
    settings.skills = skills;
    settings_store::save(&settings)
}

/// Enabled skills only — what the `skill` router searches.
pub fn enabled_catalog() -> Result<Vec<SkillMeta>, SkillError> {
    let settings = load_skills_settings().unwrap_or_default();
    let bundled = bundled_skills::bundled_metas()?;
    let user = user_skills_store::valid_user_metas()?;
    Ok(merge_catalog(bundled, user)
        .into_iter()
        .filter(|s| settings.is_enabled(s.source, &s.name))
        .collect())
}

/// Settings list: bundled + user (including invalid user folders).
pub fn list_skills() -> Result<Vec<SkillListItem>, SkillError> {
    let settings = load_skills_settings().unwrap_or_default();
    let bundled = bundled_skills::bundled_metas()?;
    let user_entries = user_skills_store::scan_user_skills()?;
    let user_names: std::collections::HashSet<&str> =
        user_entries.iter().map(|e| e.dir_name.as_str()).collect();

    let mut items = Vec::new();
    for meta in bundled {
        if user_names.contains(meta.name.as_str()) {
            continue;
        }
        items.push(SkillListItem {
            enabled: settings.is_enabled(meta.source, &meta.name),
            error: None,
            name: meta.name,
            description: meta.description,
            source: meta.source,
        });
    }
    for entry in user_entries {
        match entry.parsed {
            Ok(parsed) => items.push(SkillListItem {
                enabled: settings.is_enabled(SkillSource::User, &parsed.name),
                error: None,
                name: parsed.name,
                description: parsed.description,
                source: SkillSource::User,
            }),
            Err(err) => items.push(SkillListItem {
                enabled: false,
                error: Some(err.to_string()),
                name: entry.dir_name,
                description: String::new(),
                source: SkillSource::User,
            }),
        }
    }
    Ok(items)
}

/// Settings preview: the files of one skill, `SKILL.md` first. Read-only, and
/// deliberately unfiltered by the enable/disable setting — Settings shows
/// disabled and broken skills too, and both are worth looking inside.
pub fn preview_files(source: SkillSource, name: &str) -> Result<Vec<String>, SkillError> {
    match source {
        SkillSource::Bundled => {
            let skill = bundled_skills::bundled_skill(name)
                .ok_or_else(|| SkillError::NotFound(name.to_string()))?;
            let mut files = vec![SKILL_MD.to_string()];
            files.extend(skill.files.iter().map(|f| f.path.to_string()));
            Ok(files)
        }
        SkillSource::User => user_skills_store::preview_files(name),
    }
}

/// Settings preview: the text of one file listed by `preview_files`.
pub fn preview_file(source: SkillSource, name: &str, path: &str) -> Result<String, SkillError> {
    match source {
        SkillSource::Bundled => {
            let skill = bundled_skills::bundled_skill(name)
                .ok_or_else(|| SkillError::NotFound(name.to_string()))?;
            if path == SKILL_MD {
                return Ok(skill.skill_md.to_string());
            }
            bundled_skills::bundled_file(skill, path)
                .map(|c| c.to_string())
                .ok_or_else(|| SkillError::NotFound(format!("{name}/{path}")))
        }
        SkillSource::User => user_skills_store::preview_file(name, path),
    }
}

/// Remove a user-installed skill. Bundled skills cannot be deleted.
pub fn remove_skill(name: &str) -> Result<(), SkillError> {
    let catalog = list_skills()?;
    let Some(item) = catalog.iter().find(|s| s.name == name) else {
        return Err(SkillError::NotFound(name.to_string()));
    };
    if item.source != SkillSource::User {
        return Err(SkillError::CannotRemoveBundled);
    }
    user_skills_store::remove_user_skill(name)
}

pub fn set_skill_enabled(source: SkillSource, name: &str, enabled: bool) -> Result<(), SkillError> {
    if name.trim().is_empty() {
        return Err(SkillError::InvalidName(name.to_string(), "empty".into()));
    }
    let mut settings = load_skills_settings().unwrap_or_default();
    settings.set_enabled(source, name, enabled);
    save_skills_settings(settings).map_err(|e| SkillError::Message(e.to_string()))
}

pub fn search(query: &str) -> Result<ToolResult, SkillError> {
    let catalog = enabled_catalog()?;
    let hits = search_skills(query, &catalog)?;
    Ok(ToolResult::SkillSearch(SkillSearchResult {
        matches: hits
            .into_iter()
            .map(|s| SkillSearchHit {
                name: s.name.clone(),
                description: s.description.clone(),
                source: s.source,
            })
            .collect(),
    }))
}

pub fn load(name: &str) -> Result<ToolResult, SkillError> {
    let (meta, body, files) = load_enabled(name)?;
    Ok(ToolResult::SkillLoaded(SkillLoadedResult {
        name: meta.name,
        source: meta.source,
        body,
        files,
    }))
}

pub fn read(name: &str, path: &str) -> Result<ToolResult, SkillError> {
    let meta = enabled_meta(name)?;
    let content = match meta.source {
        SkillSource::Bundled => {
            let skill = bundled_skills::bundled_skill(name)
                .ok_or_else(|| SkillError::NotFound(name.to_string()))?;
            let content = bundled_skills::bundled_file(skill, path)
                .ok_or_else(|| SkillError::NotFound(format!("{name}/{path}")))?;
            content.to_string()
        }
        SkillSource::User => user_skills_store::read_companion(name, path)?,
    };
    Ok(ToolResult::SkillFile(SkillFileResult {
        name: name.to_string(),
        path: path.to_string(),
        content,
    }))
}

fn enabled_meta(name: &str) -> Result<SkillMeta, SkillError> {
    enabled_catalog()?
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| {
            let settings = load_skills_settings().unwrap_or_default();
            // Distinguish "disabled" from "unknown" when the name exists
            // in the unfiltered merge.
            if let Ok(all) = unfiltered_catalog() {
                if let Some(found) = all.iter().find(|s| s.name == name) {
                    if !settings.is_enabled(found.source, &found.name) {
                        return SkillError::Disabled(name.to_string());
                    }
                }
            }
            SkillError::NotFound(name.to_string())
        })
}

fn unfiltered_catalog() -> Result<Vec<SkillMeta>, SkillError> {
    let bundled = bundled_skills::bundled_metas()?;
    let user = user_skills_store::valid_user_metas()?;
    Ok(merge_catalog(bundled, user))
}

fn load_enabled(name: &str) -> Result<(SkillMeta, String, Vec<String>), SkillError> {
    let meta = enabled_meta(name)?;
    match meta.source {
        SkillSource::Bundled => {
            let skill = bundled_skills::bundled_skill(name)
                .ok_or_else(|| SkillError::NotFound(name.to_string()))?;
            let parsed = bundled_skills::parse_bundled(skill)?;
            let files = skill.files.iter().map(|f| f.path.to_string()).collect();
            Ok((meta, parsed.body, files))
        }
        SkillSource::User => {
            let (parsed, _) = user_skills_store::load_user_skill(name)?;
            let files = user_skills_store::companion_files(name)?;
            Ok((
                SkillMeta {
                    name: parsed.name,
                    description: parsed.description,
                    source: SkillSource::User,
                },
                parsed.body,
                files,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;
    use crate::infra::user_skills_store::{ensure_user_skills_dir, import_skill_dir};
    use crate::services::standards_rules::{FileEntry, MethodFolderCtx, RULES};

    /// The `method-spec` skill tells the model to run `check` on what it
    /// wrote, so the document its own template produces has to survive that
    /// check. Builds a method folder out of the bundled template and runs
    /// the real rule registry over it.
    fn template_method_folder() -> MethodFolderCtx {
        let method = "createSignOperationV2";
        let template = preview_file(SkillSource::Bundled, "method-spec", "assets/template.adoc")
            .unwrap()
            .replace("{имяРучки}", method);
        let file = |name: String, content: &str| FileEntry {
            path: std::path::PathBuf::from(&name),
            name,
            content: content.to_string(),
        };
        MethodFolderCtx::new(
            method.to_string(),
            vec![
                file(format!("{method}.adoc"), &template),
                file(
                    "request.adoc".to_string(),
                    "[discrete#endpoint]\n=== Endpoint\n\n`POST /api/documents`\n",
                ),
                file("response.adoc".to_string(), "[source,json]\n----\n{}\n----\n"),
                file(format!("{method}.puml"), "@startuml\nA -> B\n@enduml\n"),
            ],
        )
    }

    fn rule_outcome(ctx: &MethodFolderCtx, id: &str) -> bool {
        let rule = RULES.iter().find(|r| r.def.id == id).expect(id);
        (rule.check)(ctx).passed
    }

    #[test]
    fn bundled_template_passes_the_standards_checker() {
        with_temp_home(|| {
            let ctx = template_method_folder();
            let failed: Vec<&str> = RULES
                .iter()
                .filter(|rule| !(rule.check)(&ctx).passed)
                .map(|rule| rule.def.id)
                .collect();
            assert!(failed.is_empty(), "template fails rules: {failed:?}");
        });
    }

    /// A blank line between the document title and the attribute entries
    /// closes the AsciiDoc header, and `:toc:` stops applying — the document
    /// renders without its «Оглавление» at all. K.2.1 does not catch it (it
    /// only greps for the `:toc:` string), so the template is the guard.
    #[test]
    fn bundled_template_keeps_its_attributes_inside_the_document_header() {
        with_temp_home(|| {
            let template =
                preview_file(SkillSource::Bundled, "method-spec", "assets/template.adoc").unwrap();
            let lines: Vec<&str> = template.lines().collect();
            let title = lines
                .iter()
                .position(|l| l.starts_with("= "))
                .expect("template has a document title");
            assert!(
                lines[title + 1].starts_with(':'),
                "blank line after the title closes the header: {:?}",
                lines[title + 1]
            );
            // No stray `//` on the title line either — it is not a comment
            // there, it becomes part of the title.
            assert!(!lines[title].contains("//"));
        });
    }

    #[test]
    fn empty_table_cells_are_what_would_break_the_template() {
        with_temp_home(|| {
            // Guards the fix for the empty «Варианты значений» cells: K.4.2 and
            // K.5.2 count any blank cell in a 4+-column table as a failure, so
            // the template writes the house `-` placeholder instead.
            let mut ctx = template_method_folder();
            ctx.files[0].content = ctx.files[0].content.replacen("|-\n", "|\n", 1);
            assert!(!rule_outcome(&ctx, "K.4.2"));
        });
    }

    fn search_hits(query: &str) -> Vec<String> {
        let ToolResult::SkillSearch(SkillSearchResult { matches }) = search(query).unwrap() else {
            panic!("expected search result");
        };
        matches.into_iter().map(|m| m.name).collect()
    }

    /// Nothing lists the catalog for the model — a skill exists only if
    /// `search` finds it, and matching is plain substring over name +
    /// description, with no stemming. So every phrase a skill claims as its
    /// own trigger has to actually hit it, in the grammatical form a user
    /// would type.
    #[test]
    fn bundled_skills_are_found_by_the_phrases_they_claim() {
        with_temp_home(|| {
            for query in [
                "описать ручку",
                "документация на ручку",
                "оформить постановку",
                "сделать постановку на метод",
                "поправить постановку",
                "постановка на REST метод",
                "расписать флоу",
                "ТЗ на эндпоинт",
                "добавить таблицу ошибок в постановку",
                "описать вызов соседнего сервиса",
            ] {
                assert!(
                    search_hits(query).contains(&"method-spec".to_string()),
                    "method-spec not found by {query:?}"
                );
            }
            for query in [
                "openapi спецификация",
                "куда положить схему в спецификации",
                "$ref schemas",
                "swagger",
                "operations layout",
            ] {
                assert!(
                    search_hits(query).contains(&"openapi-specs-layout".to_string()),
                    "openapi-specs-layout not found by {query:?}"
                );
            }
            // Первые три — дословно те формулировки, которые системный
            // промпт обещает роутеру (`SKILLS_ROUTER_HINT` в
            // `src/lib/assistantConfig.ts`). Поиск не умеет стемминг, так
            // что обещание держится только на совпадении отдельных слов
            // («тикет», «задачу», «таск»); если описание скилла перепишут,
            // сломается здесь, а не в бою.
            for query in [
                "составь тикет",
                "оформи задачу",
                "накидай таск",
                "написать задачу в Jira",
                "acceptance criteria",
                "definition of done",
                "user story",
            ] {
                assert!(
                    search_hits(query).contains(&"jira-task-description".to_string()),
                    "jira-task-description not found by {query:?}"
                );
            }
        });
    }

    #[test]
    fn enabled_catalog_includes_bundled_by_default() {
        with_temp_home(|| {
            let catalog = enabled_catalog().unwrap();
            assert!(catalog.iter().any(|s| s.name == "method-spec"));
            assert!(catalog.iter().any(|s| s.name == "openapi-specs-layout"));
            assert!(catalog.iter().any(|s| s.name == "jira-task-description"));
        });
    }

    #[test]
    fn disabled_bundled_skill_is_absent_from_search() {
        with_temp_home(|| {
            set_skill_enabled(SkillSource::Bundled, "method-spec", false).unwrap();
            let err_or_hits = search("REST method folder documentation");
            let result = err_or_hits.unwrap();
            let ToolResult::SkillSearch(SkillSearchResult { matches }) = result else {
                panic!("expected search result");
            };
            assert!(!matches.iter().any(|m| m.name == "method-spec"));
        });
    }

    #[test]
    fn load_unknown_name_is_not_found() {
        with_temp_home(|| {
            assert!(matches!(
                load("no-such-skill").unwrap_err(),
                SkillError::NotFound(_)
            ));
        });
    }

    #[test]
    fn load_disabled_is_disabled_error() {
        with_temp_home(|| {
            set_skill_enabled(SkillSource::Bundled, "openapi-specs-layout", false).unwrap();
            assert!(matches!(
                load("openapi-specs-layout").unwrap_err(),
                SkillError::Disabled(_)
            ));
        });
    }

    #[test]
    fn user_skill_overlays_bundled_name() {
        with_temp_home(|| {
            let dir = ensure_user_skills_dir().unwrap();
            let skill_dir = dir.join("method-spec");
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(
                skill_dir.join("SKILL.md"),
                "---\nname: method-spec\ndescription: User overlay of the REST method fill guide.\n---\n# Overlay\n",
            )
            .unwrap();
            // It's already in the user dir — scan picks it up without import.
            let catalog = enabled_catalog().unwrap();
            let rest = catalog.iter().find(|s| s.name == "method-spec").unwrap();
            assert_eq!(rest.source, SkillSource::User);
            assert!(rest.description.contains("User overlay"));
        });
    }

    #[test]
    fn preview_lists_bundled_skill_md_first_and_reads_it() {
        with_temp_home(|| {
            let files = preview_files(SkillSource::Bundled, "method-spec").unwrap();
            assert_eq!(files.first().map(String::as_str), Some("SKILL.md"));
            assert!(files.iter().any(|f| f == "references/structure.md"));
            let md = preview_file(SkillSource::Bundled, "method-spec", "SKILL.md").unwrap();
            // The raw file, frontmatter included — the viewer shows the source.
            assert!(md.starts_with("---"));
        });
    }

    #[test]
    fn preview_works_for_a_disabled_skill() {
        with_temp_home(|| {
            set_skill_enabled(SkillSource::Bundled, "method-spec", false).unwrap();
            assert!(preview_file(SkillSource::Bundled, "method-spec", "SKILL.md").is_ok());
        });
    }

    #[test]
    fn preview_works_for_a_user_skill_with_broken_frontmatter() {
        with_temp_home(|| {
            let dir = ensure_user_skills_dir().unwrap();
            let skill_dir = dir.join("broken-skill");
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(skill_dir.join("SKILL.md"), "no frontmatter here\n").unwrap();
            std::fs::write(skill_dir.join("notes.md"), "companion").unwrap();

            let files = preview_files(SkillSource::User, "broken-skill").unwrap();
            assert_eq!(files, vec!["SKILL.md", "notes.md"]);
            let text = preview_file(SkillSource::User, "broken-skill", "SKILL.md").unwrap();
            assert_eq!(text, "no frontmatter here\n");
        });
    }

    #[test]
    fn preview_rejects_a_path_escape() {
        with_temp_home(|| {
            let dir = ensure_user_skills_dir().unwrap();
            let skill_dir = dir.join("my-skill");
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(
                skill_dir.join("SKILL.md"),
                "---\nname: my-skill\ndescription: A user skill.\n---\n# Body\n",
            )
            .unwrap();
            assert!(matches!(
                preview_file(SkillSource::User, "my-skill", "../../settings.json").unwrap_err(),
                SkillError::PathEscape(_)
            ));
            assert!(matches!(
                preview_files(SkillSource::User, "../..").unwrap_err(),
                SkillError::PathEscape(_)
            ));
        });
    }

    #[test]
    fn remove_bundled_is_forbidden() {
        with_temp_home(|| {
            assert!(matches!(
                remove_skill("method-spec").unwrap_err(),
                SkillError::CannotRemoveBundled
            ));
        });
    }

    #[test]
    fn import_without_skill_md_fails() {
        with_temp_home(|| {
            let tmp = std::env::temp_dir().join(format!(
                "atlas-skill-svc-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let empty = tmp.join("not-a-skill");
            std::fs::create_dir_all(&empty).unwrap();
            let err = import_skill_dir(&empty).unwrap_err();
            std::fs::remove_dir_all(&tmp).ok();
            assert!(err.to_string().contains("SKILL.md"));
        });
    }
}
