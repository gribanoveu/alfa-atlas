//! Agent Skills (https://agentskills.io/specification): parse `SKILL.md`,
//! rank a catalog by query, and persist enable/disable overrides.
//!
//! No I/O — scanning `~/.atlas/skills` and the bundled registry live in
//! `infra`/`services`. This module is the typed core the router and Settings
//! share.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SEARCH_TOP_K: usize = 6;
pub const NAME_MAX_CHARS: usize = 64;
pub const DESCRIPTION_MAX_CHARS: usize = 1024;

/// Where a skill was loaded from. User skills of the same `name` overlay
/// bundled ones at merge time (`services::agent_skills`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillSource {
    Bundled,
    User,
}

impl SkillSource {
    pub fn as_key(self) -> &'static str {
        match self {
            SkillSource::Bundled => "bundled",
            SkillSource::User => "user",
        }
    }
}

/// Persistence key for `SkillsSettings::disabled`: `"bundled:name"` /
/// `"user:name"`.
pub fn disabled_key(source: SkillSource, name: &str) -> String {
    format!("{}:{name}", source.as_key())
}

/// Global enable/disable overrides in `AppSettings`. A skill absent from
/// `disabled` is on — same opt-out shape as `StandardsRuleConfig`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillsSettings {
    #[serde(default)]
    pub disabled: Vec<String>,
}

impl SkillsSettings {
    pub fn is_enabled(&self, source: SkillSource, name: &str) -> bool {
        !self.disabled.iter().any(|k| k == &disabled_key(source, name))
    }

    pub fn set_enabled(&mut self, source: SkillSource, name: &str, enabled: bool) {
        let key = disabled_key(source, name);
        self.disabled.retain(|k| k != &key);
        if !enabled {
            self.disabled.push(key);
        }
    }
}

/// Discovery metadata — what `skill` search returns and Settings lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
}

/// One row in the Settings list — includes invalid user folders so the UI
/// can show them greyed out. `error` is set when `SKILL.md` failed to parse;
/// those rows never enter the router catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillListItem {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub enabled: bool,
    pub error: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SkillError {
    #[error("SKILL.md is missing YAML frontmatter")]
    MissingFrontmatter,
    #[error("invalid SKILL.md frontmatter: {0}")]
    InvalidFrontmatter(String),
    #[error("skill name {0:?} is invalid: {1}")]
    InvalidName(String, String),
    #[error("skill name {0:?} does not match directory name {1:?}")]
    NameMismatch(String, String),
    #[error("description must be 1–{DESCRIPTION_MAX_CHARS} characters")]
    InvalidDescription,
    #[error("skill not found: {0}")]
    NotFound(String),
    #[error("skill search requires a non-empty query")]
    EmptyQuery,
    #[error("unknown skill op {0:?} (expected search, load, or read)")]
    UnknownOp(String),
    #[error("skill {0:?} is disabled")]
    Disabled(String),
    #[error("path escapes skill root: {0}")]
    PathEscape(String),
    #[error("cannot remove a bundled skill")]
    CannotRemoveBundled,
    #[error("a skill named {0:?} already exists")]
    AlreadyExists(String),
    #[error("{0}")]
    Message(String),
}

/// YAML frontmatter as the spec defines it. Optional fields are accepted
/// and ignored at runtime (`allowed-tools` is experimental; we have our
/// own tool allowlist).
#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    #[allow(dead_code)]
    license: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    compatibility: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    metadata: Option<std::collections::HashMap<String, String>>,
    #[serde(default, rename = "allowed-tools")]
    #[allow(dead_code)]
    allowed_tools: Option<String>,
}

/// Parsed `SKILL.md`: validated meta plus markdown body (frontmatter stripped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSkill {
    pub name: String,
    pub description: String,
    pub body: String,
}

/// Spec `name` field: 1–64 chars, `[a-z0-9-]`, no leading/trailing/
/// consecutive hyphens.
pub fn validate_skill_name(name: &str) -> Result<(), SkillError> {
    if name.is_empty() || name.chars().count() > NAME_MAX_CHARS {
        return Err(SkillError::InvalidName(
            name.to_string(),
            format!("must be 1–{NAME_MAX_CHARS} characters"),
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(SkillError::InvalidName(
            name.to_string(),
            "must not start or end with a hyphen".to_string(),
        ));
    }
    if name.contains("--") {
        return Err(SkillError::InvalidName(
            name.to_string(),
            "must not contain consecutive hyphens".to_string(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(SkillError::InvalidName(
            name.to_string(),
            "only lowercase letters, digits, and hyphens are allowed".to_string(),
        ));
    }
    Ok(())
}

fn validate_description(description: &str) -> Result<(), SkillError> {
    let len = description.chars().count();
    if len == 0 || len > DESCRIPTION_MAX_CHARS {
        return Err(SkillError::InvalidDescription);
    }
    Ok(())
}

/// Split `SKILL.md` on YAML frontmatter (`---` fences) and validate `name`
/// against both the spec and the parent directory name.
pub fn parse_skill_md(contents: &str, dir_name: &str) -> Result<ParsedSkill, SkillError> {
    let (yaml, body) = split_frontmatter(contents).ok_or(SkillError::MissingFrontmatter)?;
    let front: SkillFrontmatter = serde_yaml::from_str(yaml)
        .map_err(|e| SkillError::InvalidFrontmatter(e.to_string()))?;
    validate_skill_name(&front.name)?;
    validate_description(&front.description)?;
    if front.name != dir_name {
        return Err(SkillError::NameMismatch(front.name, dir_name.to_string()));
    }
    Ok(ParsedSkill {
        name: front.name,
        description: front.description,
        body: body.to_string(),
    })
}

fn split_frontmatter(contents: &str) -> Option<(&str, &str)> {
    let text = contents.trim_start_matches('\u{feff}');
    let rest = text.strip_prefix("---")?;
    let rest = rest.strip_prefix('\r').unwrap_or(rest).strip_prefix('\n')?;
    let close = rest.find("\n---").or_else(|| rest.find("\r\n---"))?;
    let yaml = &rest[..close];
    let after = &rest[close..];
    let after = after
        .strip_prefix("\r\n---")
        .or_else(|| after.strip_prefix("\n---"))?;
    let body = after
        .strip_prefix("\r\n")
        .or_else(|| after.strip_prefix('\n'))
        .unwrap_or(after);
    Some((yaml, body))
}

/// Rank `skills` against a free-form query. Empty/whitespace queries are
/// rejected — the router must not dump the full catalog into context.
pub fn search_skills<'a>(
    query: &str,
    skills: &'a [SkillMeta],
) -> Result<Vec<&'a SkillMeta>, SkillError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(SkillError::EmptyQuery);
    }
    let tokens = tokenize(query);
    if tokens.is_empty() {
        return Err(SkillError::EmptyQuery);
    }
    let query_lower = query.to_lowercase();
    let mut scored: Vec<(&SkillMeta, u32)> = skills
        .iter()
        .filter_map(|skill| {
            let score = score_skill(&query_lower, &tokens, skill);
            (score > 0).then_some((skill, score))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
    Ok(scored
        .into_iter()
        .take(SEARCH_TOP_K)
        .map(|(s, _)| s)
        .collect())
}

fn tokenize(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in query.chars() {
        if c.is_alphanumeric() {
            current.extend(c.to_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn score_skill(query_lower: &str, tokens: &[String], skill: &SkillMeta) -> u32 {
    let name_lower = skill.name.to_lowercase();
    let desc_lower = skill.description.to_lowercase();
    let mut score = 0u32;
    if name_lower == query_lower || name_lower.replace('-', " ") == query_lower {
        score += 1000;
    }
    for token in tokens {
        if name_lower == *token {
            score += 40;
        } else if name_lower.contains(token.as_str()) {
            score += 10;
        }
        if desc_lower.contains(token.as_str()) {
            score += 3;
        }
    }
    score
}

/// User entries overlay bundled ones of the same `name`. Relative order:
/// remaining bundled skills first (original order), then user-only skills.
pub fn merge_catalog(bundled: Vec<SkillMeta>, user: Vec<SkillMeta>) -> Vec<SkillMeta> {
    let user_names: std::collections::HashSet<&str> =
        user.iter().map(|s| s.name.as_str()).collect();
    let mut out: Vec<SkillMeta> = bundled
        .into_iter()
        .filter(|s| !user_names.contains(s.name.as_str()))
        .collect();
    out.extend(user);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(name: &str, description: &str, body: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n{body}")
    }

    fn meta(name: &str, description: &str, source: SkillSource) -> SkillMeta {
        SkillMeta {
            name: name.to_string(),
            description: description.to_string(),
            source,
        }
    }

    #[test]
    fn parse_skill_md_accepts_minimal_frontmatter() {
        let parsed = parse_skill_md(&md("code-review", "Reviews PRs when asked to review.", "# Hi\n"), "code-review")
            .unwrap();
        assert_eq!(parsed.name, "code-review");
        assert_eq!(parsed.description, "Reviews PRs when asked to review.");
        assert_eq!(parsed.body, "# Hi\n");
    }

    #[test]
    fn parse_skill_md_rejects_missing_frontmatter() {
        assert_eq!(
            parse_skill_md("# just markdown\n", "just-markdown").unwrap_err(),
            SkillError::MissingFrontmatter
        );
    }

    #[test]
    fn parse_skill_md_rejects_name_directory_mismatch() {
        let err = parse_skill_md(&md("code-review", "Reviews PRs when asked to review.", ""), "other").unwrap_err();
        assert_eq!(
            err,
            SkillError::NameMismatch("code-review".into(), "other".into())
        );
    }

    #[test]
    fn validate_skill_name_rejects_uppercase_and_double_hyphen() {
        assert!(validate_skill_name("PDF-Processing").is_err());
        assert!(validate_skill_name("-pdf").is_err());
        assert!(validate_skill_name("pdf-").is_err());
        assert!(validate_skill_name("pdf--processing").is_err());
        assert!(validate_skill_name("code-review").is_ok());
    }

    #[test]
    fn parse_skill_md_rejects_empty_description() {
        let raw = "---\nname: code-review\ndescription: \"\"\n---\nbody\n";
        assert_eq!(
            parse_skill_md(raw, "code-review").unwrap_err(),
            SkillError::InvalidDescription
        );
    }

    #[test]
    fn parse_skill_md_ignores_optional_spec_fields() {
        let raw = "---\nname: pdf-processing\ndescription: Extract PDF text when handling PDFs.\nlicense: Apache-2.0\nmetadata:\n  author: example\n---\n# Body\n";
        let parsed = parse_skill_md(raw, "pdf-processing").unwrap();
        assert_eq!(parsed.body, "# Body\n");
    }

    #[test]
    fn search_skills_rejects_empty_query() {
        let catalog = vec![meta("rest-endpoint-docs", "Fill REST method docs", SkillSource::Bundled)];
        assert_eq!(search_skills("   ", &catalog).unwrap_err(), SkillError::EmptyQuery);
    }

    #[test]
    fn search_skills_ranks_name_match_above_description() {
        let catalog = vec![
            meta(
                "openapi-specs-layout",
                "OpenAPI multi-file specs, schemas, $ref",
                SkillSource::Bundled,
            ),
            meta(
                "rest-endpoint-docs",
                "Fill a REST API method documentation folder after scaffold",
                SkillSource::Bundled,
            ),
        ];
        let hits = search_skills("rest endpoint method folder", &catalog).unwrap();
        assert_eq!(hits[0].name, "rest-endpoint-docs");
    }

    #[test]
    fn search_skills_does_not_return_unrelated_skills() {
        let catalog = vec![meta(
            "openapi-specs-layout",
            "OpenAPI schemas operations $ref",
            SkillSource::Bundled,
        )];
        let hits = search_skills("unrelated-zzzz", &catalog).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn search_skills_caps_at_top_k() {
        let catalog: Vec<SkillMeta> = (0..10)
            .map(|i| meta(&format!("skill-{i}"), "documentation method rest api", SkillSource::User))
            .collect();
        let hits = search_skills("documentation rest", &catalog).unwrap();
        assert_eq!(hits.len(), SEARCH_TOP_K);
    }

    #[test]
    fn skills_settings_defaults_enabled_and_can_disable() {
        let mut settings = SkillsSettings::default();
        assert!(settings.is_enabled(SkillSource::Bundled, "rest-endpoint-docs"));
        settings.set_enabled(SkillSource::Bundled, "rest-endpoint-docs", false);
        assert!(!settings.is_enabled(SkillSource::Bundled, "rest-endpoint-docs"));
        assert!(settings.is_enabled(SkillSource::User, "rest-endpoint-docs"));
        settings.set_enabled(SkillSource::Bundled, "rest-endpoint-docs", true);
        assert!(settings.is_enabled(SkillSource::Bundled, "rest-endpoint-docs"));
        assert!(settings.disabled.is_empty());
    }

    #[test]
    fn merge_catalog_user_overrides_bundled_same_name() {
        let bundled = vec![meta("shared", "bundled description of shared skill", SkillSource::Bundled)];
        let user = vec![meta("shared", "user description of shared skill", SkillSource::User)];
        let merged = merge_catalog(bundled, user);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source, SkillSource::User);
        assert_eq!(merged[0].description, "user description of shared skill");
    }

    #[test]
    fn merge_catalog_keeps_distinct_names() {
        let bundled = vec![meta("a-skill", "bundled a", SkillSource::Bundled)];
        let user = vec![meta("b-skill", "user b", SkillSource::User)];
        let merged = merge_catalog(bundled, user);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "a-skill");
        assert_eq!(merged[1].name, "b-skill");
    }
}
