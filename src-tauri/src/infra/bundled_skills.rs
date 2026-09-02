//! Compile-time registry of Agent Skills shipped with the app.
//!
//! Each skill is a `SKILL.md` under `src-tauri/assets/skills/<name>/`,
//! loaded with `include_str!` the same way `llm_provider_manifest` bakes in
//! `system_providers.yaml`. Companion files (if any) are listed beside the
//! markdown; v1 bundled skills are instruction-only.

use crate::domain::agent_skills::{parse_skill_md, ParsedSkill, SkillError, SkillMeta, SkillSource};

pub struct BundledFile {
    pub path: &'static str,
    pub content: &'static str,
}

pub struct BundledSkill {
    pub name: &'static str,
    pub skill_md: &'static str,
    pub files: &'static [BundledFile],
}

const OPENAPI_SPECS_LAYOUT: BundledSkill = BundledSkill {
    name: "openapi-specs-layout",
    skill_md: include_str!("../../assets/skills/openapi-specs-layout/SKILL.md"),
    files: &[],
};

const METHOD_SPEC: BundledSkill = BundledSkill {
    name: "method-spec",
    skill_md: include_str!("../../assets/skills/method-spec/SKILL.md"),
    files: &[
        BundledFile {
            path: "references/structure.md",
            content: include_str!("../../assets/skills/method-spec/references/structure.md"),
        },
        BundledFile {
            path: "references/errors.md",
            content: include_str!("../../assets/skills/method-spec/references/errors.md"),
        },
        BundledFile {
            path: "references/glossary.md",
            content: include_str!("../../assets/skills/method-spec/references/glossary.md"),
        },
        BundledFile {
            path: "references/tools.md",
            content: include_str!("../../assets/skills/method-spec/references/tools.md"),
        },
        BundledFile {
            path: "assets/template.adoc",
            content: include_str!("../../assets/skills/method-spec/assets/template.adoc"),
        },
    ],
};

const JIRA_TASK_DESCRIPTION: BundledSkill = BundledSkill {
    name: "jira-task-description",
    skill_md: include_str!("../../assets/skills/jira-task-description/SKILL.md"),
    files: &[],
};

pub const BUNDLED_SKILLS: &[BundledSkill] =
    &[OPENAPI_SPECS_LAYOUT, METHOD_SPEC, JIRA_TASK_DESCRIPTION];

pub fn bundled_skill(name: &str) -> Option<&'static BundledSkill> {
    BUNDLED_SKILLS.iter().find(|s| s.name == name)
}

pub fn parse_bundled(skill: &BundledSkill) -> Result<ParsedSkill, SkillError> {
    parse_skill_md(skill.skill_md, skill.name)
}

pub fn bundled_metas() -> Result<Vec<SkillMeta>, SkillError> {
    BUNDLED_SKILLS
        .iter()
        .map(|s| {
            let parsed = parse_bundled(s)?;
            Ok(SkillMeta {
                name: parsed.name,
                description: parsed.description,
                source: SkillSource::Bundled,
            })
        })
        .collect()
}

pub fn bundled_file<'a>(skill: &'a BundledSkill, path: &str) -> Option<&'a str> {
    skill
        .files
        .iter()
        .find(|f| f.path == path)
        .map(|f| f.content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_skill_parses_and_name_matches_directory() {
        assert!(!BUNDLED_SKILLS.is_empty());
        for skill in BUNDLED_SKILLS {
            let parsed = parse_bundled(skill).expect(skill.name);
            assert_eq!(parsed.name, skill.name);
            assert!(!parsed.description.is_empty());
            assert!(!parsed.body.is_empty());
        }
    }
}
