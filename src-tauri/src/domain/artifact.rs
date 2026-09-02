//! Artifacts — structured documents the *user* fills in, at the assistant's
//! request, when the repository simply does not contain the facts a document
//! needs (the canonical case: the «Входные параметры» table of a REST method
//! spec, where nothing on disk says what the request actually looks like).
//!
//! Stored under `~/.atlas/artifacts/{repository_id}/{artifact_id}.json` —
//! same repository-identity keying as plans and the embeddings cache, and
//! deliberately outside the repo: an artifact is working material for
//! writing documentation, not documentation itself.
//!
//! The kind/content split is the extension point. `ArtifactKind` names the
//! shape, `ArtifactContent` carries it, and everything else in the record is
//! kind-agnostic — a second kind adds one variant to each and one renderer,
//! and touches nothing in the store, the tool loop, or the chat card.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Which shape an artifact's content takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactKind {
    HttpRequest,
    JiraTicket,
}

impl ArtifactKind {
    /// Whether the assistant may author and rewrite this kind itself
    /// (`artifact` `op: "create"`/`"update"`), rather than only asking the
    /// user to fill it in.
    ///
    /// `HttpRequest` is deliberately excluded. It exists precisely because
    /// the request/response shape of a method is *not* in the repository —
    /// letting the model write one would produce exactly the plausible,
    /// invented parameter table that `requestArtifact` was introduced to
    /// prevent. A ticket is the opposite case: its content is the model's
    /// own prose, synthesized from what the user just said, and there is
    /// nothing to fabricate.
    pub fn is_agent_authored(self) -> bool {
        match self {
            ArtifactKind::HttpRequest => false,
            ArtifactKind::JiraTicket => true,
        }
    }
}

/// `Draft` while the user is still filling it in; `Ready` once they pressed
/// «Отправить ассистенту». Only `Ready` artifacts are advertised to the
/// model in the per-turn context block — a half-filled draft is noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactStatus {
    Draft,
    Ready,
}

/// One row of a documentation parameter table. The five fields are exactly
/// the five columns of the REST template's tables (`Параметр | Формат |
/// Обязательность | Описание | Варианты значений`, see
/// `src/templates/asciidoc/rest-endpoint/methodName.adoc`) — keeping them
/// aligned is what makes `artifact_render` a straight projection rather
/// than a transformation with judgement calls in it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamSpec {
    pub name: String,
    /// «Формат» column — `string`, `integer`, `object`, …
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: String,
    /// «Варианты значений» column. Empty renders as `-`, per the template.
    #[serde(default)]
    pub values: String,
}

/// Request body: the media type, a literal example, and the field rows the
/// example was described with. `params` is not derived from `sample` at
/// render time — the user edits the rows directly (the builder can seed
/// them from the sample, but the two are independent afterwards, because a
/// documented field list routinely says more than an example shows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BodySpec {
    #[serde(default = "default_media_type")]
    pub media_type: String,
    #[serde(default)]
    pub sample: String,
    #[serde(default)]
    pub params: Vec<ParamSpec>,
}

fn default_media_type() -> String {
    "application/json".to_string()
}

impl Default for BodySpec {
    fn default() -> Self {
        Self {
            media_type: default_media_type(),
            sample: String::new(),
            params: Vec::new(),
        }
    }
}

fn default_base_url() -> String {
    "https://{host}".to_string()
}

impl Default for HttpRequestSpec {
    fn default() -> Self {
        Self {
            method: String::new(),
            base_url: default_base_url(),
            path: String::new(),
            path_params: Vec::new(),
            query_params: Vec::new(),
            headers: Vec::new(),
            body: None,
            responses: Vec::new(),
            errors: Vec::new(),
            notes: None,
        }
    }
}

/// One documented response. `status` is a string rather than a number so
/// `"2xx"` and `"200 (успех)"` stay expressible — this is documentation
/// copy, not a wire value.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseSpec {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub sample: String,
    #[serde(default)]
    pub params: Vec<ParamSpec>,
}

/// One row of the «Возможные ошибки» table.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorSpec {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub description: String,
}

/// Everything the HTTP-request designer collects. Every field defaults, so
/// a partial `prefill` from the model deserializes into a usable draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestSpec {
    #[serde(default)]
    pub method: String,
    /// Defaults to the house placeholder rather than empty — the REST
    /// endpoint convention documented in the `method-spec` skill
    /// (`references/structure.md`) is `https://{host}/<сервис>/<путь>/...`,
    /// and a blank field would otherwise produce a document with no
    /// endpoint token at all until the user notices and fills it in.
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub path_params: Vec<ParamSpec>,
    #[serde(default)]
    pub query_params: Vec<ParamSpec>,
    #[serde(default)]
    pub headers: Vec<ParamSpec>,
    #[serde(default)]
    pub body: Option<BodySpec>,
    #[serde(default)]
    pub responses: Vec<ResponseSpec>,
    #[serde(default)]
    pub errors: Vec<ErrorSpec>,
    /// Free-form note rendered as the template's `NOTE:` admonition.
    #[serde(default)]
    pub notes: Option<String>,
}

/// One entry of the ticket's «Ссылки» block. `kind` is free text (`GIT`,
/// `CONFLUENCE`, `FIGMA`, …) because Jira's own link types are per-instance
/// and this list is copy material, not an API payload.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketLink {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
}

/// A Jira ticket description. The field order *is* the section order the
/// `jira-task-description` skill mandates, and every field defaults so a
/// partially-filled ticket from the model deserializes rather than failing
/// the whole call.
///
/// Empty fields are not rendered at all — per the same skill, a heading with
/// nothing under it is worse than a missing one. That is why nothing here is
/// `Option`: "absent" and "empty" mean the same thing for a ticket section,
/// and one representation avoids the model having to choose between `null`
/// and `""`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JiraTicketSpec {
    /// The issue this draft became, once published — e.g. `WOWTAX-123`.
    ///
    /// Written by `services::jira_publish`, never by hand and never by the
    /// model: it is the *result* of publishing, and it is what stops a
    /// second publish from creating a duplicate issue. Identity, not
    /// content, so it is outside the section order below and is never
    /// rendered into the description.
    pub issue_key: String,
    /// «Почему задача существует» — the problem, without the solution.
    pub why: String,
    /// «Что должно измениться» — target state as «Пользователь может …».
    pub outcome: String,
    pub in_scope: Vec<String>,
    pub out_of_scope: Vec<String>,
    /// «Техническое решение» — endpoint/approach, when known.
    pub solution: String,
    /// Checklist items; rendered as `- [ ]`.
    pub acceptance_criteria: Vec<String>,
    pub definition_of_done: Vec<String>,
    pub risks: Vec<String>,
    pub links: Vec<TicketLink>,
}

/// The kind-specific payload. Internally tagged so the JSON on disk is
/// self-describing and a future kind can be added without a migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ArtifactContent {
    HttpRequest(HttpRequestSpec),
    JiraTicket(JiraTicketSpec),
}

impl ArtifactContent {
    pub fn kind(&self) -> ArtifactKind {
        match self {
            ArtifactContent::HttpRequest(_) => ArtifactKind::HttpRequest,
            ArtifactContent::JiraTicket(_) => ArtifactKind::JiraTicket,
        }
    }

    /// Empty content for a fresh draft of `kind`.
    pub fn empty_for(kind: ArtifactKind) -> Self {
        match kind {
            ArtifactKind::HttpRequest => ArtifactContent::HttpRequest(HttpRequestSpec::default()),
            ArtifactKind::JiraTicket => ArtifactContent::JiraTicket(JiraTicketSpec::default()),
        }
    }
}

/// Full on-disk artifact record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRecord {
    pub id: String,
    pub kind: ArtifactKind,
    pub title: String,
    /// Why the assistant asked for it — kept verbatim from
    /// `requestArtifact`'s `purpose`, so the builder can show the user what
    /// this is for without the chat being open.
    #[serde(default)]
    pub purpose: Option<String>,
    pub status: ArtifactStatus,
    pub content: ArtifactContent,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// The chat that requested it, when one did. Never used to *scope*
    /// access — any chat may read any artifact — only for provenance.
    #[serde(default)]
    pub chat_id: Option<String>,
    #[serde(default)]
    pub repo_root: Option<String>,
}

/// Listing row — enough for the artifacts list and the per-turn context
/// block, without loading samples and parameter tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummary {
    pub id: String,
    pub kind: ArtifactKind,
    pub title: String,
    pub status: ArtifactStatus,
    /// One-line "what's in it" — for an HTTP request, `POST /v1/documents`.
    pub subtitle: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl ArtifactRecord {
    pub fn to_summary(&self) -> ArtifactSummary {
        ArtifactSummary {
            id: self.id.clone(),
            kind: self.kind,
            title: self.title.clone(),
            status: self.status,
            subtitle: self.subtitle(),
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }

    fn subtitle(&self) -> String {
        match &self.content {
            ArtifactContent::HttpRequest(spec) => {
                let method = spec.method.trim();
                let path = spec.path.trim();
                match (method.is_empty(), path.is_empty()) {
                    (true, true) => String::new(),
                    (true, false) => path.to_string(),
                    (false, true) => method.to_uppercase(),
                    (false, false) => format!("{} {}", method.to_uppercase(), path),
                }
            }
            // The target state, since that is the one line that says what
            // the ticket is *for*; «Почему» describes the problem and reads
            // as a worse label in a list. Falls back to the problem when the
            // outcome has not been written yet.
            ArtifactContent::JiraTicket(spec) => {
                let outcome = spec.outcome.trim();
                if outcome.is_empty() {
                    first_line(spec.why.trim())
                } else {
                    first_line(outcome)
                }
            }
        }
    }
}

/// Summary text is a single-line label; a multi-paragraph section would
/// otherwise break the list row.
fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
}

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("settings error: {0}")]
    Settings(#[from] crate::domain::settings::SettingsError),
    #[error("project error: {0}")]
    Project(String),
    #[error("artifact not found: {0}")]
    NotFound(String),
    #[error("invalid artifact: {0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> HttpRequestSpec {
        HttpRequestSpec {
            method: "post".into(),
            base_url: "https://corp-gateway-test".into(),
            path: "/api/{organizationId}/documents".into(),
            path_params: vec![ParamSpec {
                name: "organizationId".into(),
                format: "string".into(),
                required: true,
                description: "Идентификатор организации".into(),
                values: "UBBWQQ".into(),
            }],
            ..Default::default()
        }
    }

    fn sample_record() -> ArtifactRecord {
        ArtifactRecord {
            id: "abc".into(),
            kind: ArtifactKind::HttpRequest,
            title: "Создание документа".into(),
            purpose: Some("Нужны входные параметры".into()),
            status: ArtifactStatus::Ready,
            content: ArtifactContent::HttpRequest(sample_spec()),
            created_at_ms: 1,
            updated_at_ms: 2,
            chat_id: Some("chat-1".into()),
            repo_root: Some("/repo".into()),
        }
    }

    #[test]
    fn record_round_trips_through_json() {
        let record = sample_record();
        let json = serde_json::to_string(&record).expect("serialize");
        let back: ArtifactRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(record, back);
    }

    #[test]
    fn content_is_internally_tagged_by_kind() {
        let json = serde_json::to_value(ArtifactContent::HttpRequest(HttpRequestSpec::default()))
            .expect("serialize");
        assert_eq!(json.get("kind").and_then(|k| k.as_str()), Some("httpRequest"));
    }

    /// The rule that keeps the model from inventing an endpoint's parameter
    /// table — see `ArtifactKind::is_agent_authored`.
    #[test]
    fn only_the_ticket_kind_may_be_authored_by_the_assistant() {
        assert!(ArtifactKind::JiraTicket.is_agent_authored());
        assert!(!ArtifactKind::HttpRequest.is_agent_authored());
    }

    /// The key is written only by publishing, so a rendered ticket must not
    /// carry it — it is identity, not a section.
    #[test]
    fn the_issue_key_is_never_rendered_into_the_description() {
        let wiki = crate::domain::artifact_render::render_jira_ticket(&JiraTicketSpec {
            issue_key: "ABC-123".into(),
            why: "Проблема".into(),
            ..Default::default()
        })
        .wiki;
        assert!(!wiki.contains("ABC-123"), "issue key leaked into the ticket: {wiki}");
    }

    #[test]
    fn a_partial_ticket_deserializes_with_defaults() {
        // What a model realistically sends first: the two prose sections and
        // a couple of criteria, nothing else.
        let spec: JiraTicketSpec = serde_json::from_str(
            r#"{"why":"Нет отчёта","acceptanceCriteria":["Пользователь видит отчёт"]}"#,
        )
        .expect("deserialize");
        assert_eq!(spec.why, "Нет отчёта");
        assert_eq!(spec.acceptance_criteria.len(), 1);
        assert!(spec.outcome.is_empty());
        assert!(spec.links.is_empty());
    }

    #[test]
    fn ticket_content_is_internally_tagged_by_kind() {
        let json = serde_json::to_value(ArtifactContent::JiraTicket(JiraTicketSpec::default()))
            .expect("serialize");
        assert_eq!(json.get("kind").and_then(|k| k.as_str()), Some("jiraTicket"));
    }

    #[test]
    fn ticket_subtitle_prefers_the_outcome_and_stays_one_line() {
        let record = |spec: JiraTicketSpec| ArtifactRecord {
            id: "t".into(),
            kind: ArtifactKind::JiraTicket,
            title: "Тикет".into(),
            purpose: None,
            status: ArtifactStatus::Ready,
            content: ArtifactContent::JiraTicket(spec),
            created_at_ms: 0,
            updated_at_ms: 0,
            chat_id: None,
            repo_root: None,
        };

        let filled = record(JiraTicketSpec {
            why: "Проблема".into(),
            outcome: "Пользователь может X\nвторая строка".into(),
            ..Default::default()
        });
        assert_eq!(filled.to_summary().subtitle, "Пользователь может X");

        // Before the outcome is written, the problem is the only label there
        // is — better than a blank row in the artifacts list.
        let early = record(JiraTicketSpec { why: "Проблема".into(), ..Default::default() });
        assert_eq!(early.to_summary().subtitle, "Проблема");
    }

    #[test]
    fn partial_spec_deserializes_with_defaults() {
        // What a model `prefill` realistically looks like: a couple of
        // fields it already knows, nothing else.
        let spec: HttpRequestSpec =
            serde_json::from_str(r#"{"method":"GET","path":"/v1/ping"}"#).expect("deserialize");
        assert_eq!(spec.method, "GET");
        assert_eq!(spec.path, "/v1/ping");
        assert!(spec.query_params.is_empty());
        assert!(spec.body.is_none());
    }

    #[test]
    fn a_fresh_spec_defaults_the_host_placeholder() {
        assert_eq!(HttpRequestSpec::default().base_url, "https://{host}");
    }

    #[test]
    fn a_prefill_missing_base_url_still_gets_the_placeholder() {
        // Same case as `partial_spec_deserializes_with_defaults` — the
        // model naming only `method`/`path` must not leave the field truly
        // blank, or the generated document has no endpoint token at all.
        let spec: HttpRequestSpec =
            serde_json::from_str(r#"{"method":"GET","path":"/v1/ping"}"#).expect("deserialize");
        assert_eq!(spec.base_url, "https://{host}");
    }

    #[test]
    fn an_explicit_empty_base_url_is_not_overridden() {
        // A user who deliberately clears the field must have that respected
        // on the next load — the default only fills a field that was never
        // set, not one set to empty.
        let spec: HttpRequestSpec =
            serde_json::from_str(r#"{"baseUrl":""}"#).expect("deserialize");
        assert_eq!(spec.base_url, "");
    }

    #[test]
    fn summary_subtitle_is_method_and_path() {
        assert_eq!(
            sample_record().to_summary().subtitle,
            "POST /api/{organizationId}/documents"
        );
    }

    #[test]
    fn summary_subtitle_tolerates_an_empty_draft() {
        let mut record = sample_record();
        record.content = ArtifactContent::empty_for(ArtifactKind::HttpRequest);
        assert_eq!(record.to_summary().subtitle, "");
    }
}
