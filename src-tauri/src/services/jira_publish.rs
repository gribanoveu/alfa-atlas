//! Publishing a ticket artifact as a real Jira issue.
//!
//! The whole sequence lives here because its steps are not independent: the
//! issue must exist before its links can be attached, and its key must be
//! written back into the artifact before anything else can know the draft
//! is spent. Splitting them across callers would let a caller do two of the
//! three and leave an issue nobody can find again.
//!
//! Ordering matters after the issue is created. The key is stored *first*,
//! links second: a failure while attaching links is a nuisance the user can
//! retry, whereas losing the key would orphan a real issue in a shared
//! tracker and invite a duplicate on the next click.

use crate::domain::artifact::{ArtifactContent, JiraTicketSpec};
use crate::domain::artifact_render;
use crate::domain::jira::{JiraCreatedIssue, JiraError, JiraLinkOutcome, JiraWebLink, NewIssue};
use crate::infra::{jira_client, llm_provider_manifest};
use crate::services::{artifacts, jira_config};

/// What one publish did, in the order it happened.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishOutcome {
    pub issue: JiraCreatedIssue,
    /// Per link, since a link failing is not the publish failing — the issue
    /// exists either way, and the user needs to know which links to retry.
    pub links: Vec<JiraLinkOutcome>,
}

/// Creates the issue, records its key on the artifact, and attaches the
/// ticket's links to it as Web Links.
///
/// Refuses an artifact that already carries a key: a second call would
/// create a second issue with the same content, and there is no undo for
/// that in a tracker the whole team reads.
pub fn publish(artifact_id: &str) -> Result<PublishOutcome, JiraError> {
    let record = artifacts::get(artifact_id).map_err(|e| JiraError::Artifact(e.to_string()))?;
    let ArtifactContent::JiraTicket(spec) = record.content.clone() else {
        return Err(JiraError::Artifact(
            "публиковать в Jira можно только тикет".to_string(),
        ));
    };
    if !spec.issue_key.trim().is_empty() {
        return Err(JiraError::AlreadyPublished(spec.issue_key.trim().to_string()));
    }

    let stored = jira_config::load_jira_settings().map_err(|e| JiraError::Settings(e.to_string()))?;
    let settings = jira_config::resolve(&stored, llm_provider_manifest::jira_preset());
    if !settings.is_addressable() {
        return Err(JiraError::NotConfigured);
    }
    if settings.project_key.trim().is_empty() {
        return Err(JiraError::MissingProject);
    }
    if settings.issue_type_id.trim().is_empty() {
        return Err(JiraError::MissingIssueType);
    }

    let summary = record.title.trim();
    if summary.is_empty() {
        return Err(JiraError::MissingSummary);
    }
    let description = artifact_render::render_jira_ticket(&spec).wiki;
    // An empty description means every section was blank. Publishing that
    // produces an issue with a title and nothing else, which is worse than
    // refusing — and there is no undo.
    if description.trim().is_empty() {
        return Err(JiraError::EmptyDescription);
    }

    let jira = jira_config::connect_stored()?;
    // The reporter is required by this instance's create screen and has no
    // default; the authenticated user is the only value that never needs
    // extra permission.
    let reporter = jira_client::current_user(&jira).ok().and_then(|u| u.account_id);

    let key = jira_client::create_issue(
        &jira,
        &NewIssue {
            project_key: settings.project_key.clone(),
            issue_type_id: settings.issue_type_id.clone(),
            summary: summary.to_string(),
            description,
            reporter,
        },
    )?;

    let issue = JiraCreatedIssue {
        url: browse_url(&settings.base_url, &key),
        key: key.clone(),
    };

    // Before the links: an issue whose key was never recorded is an issue
    // the user cannot find and will publish again.
    remember_issue_key(artifact_id, spec, &key)?;

    let links: Vec<JiraWebLink> = record_links(&record.content);
    let outcomes = if links.is_empty() {
        Vec::new()
    } else {
        jira_config::attach_web_links(&key, &links)?
    };

    Ok(PublishOutcome { issue, links: outcomes })
}

/// `{base}/browse/{KEY}` — the page a person opens. Not the `self` link from
/// the response, which points into `/rest/api/...` and is not a page.
///
/// Public because reopening a published artifact needs the same link and
/// must not rebuild it in TypeScript: the rule lives in one place, the way
/// `artifact_render` does.
pub fn issue_url(key: &str) -> Option<String> {
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let stored = jira_config::load_jira_settings().ok()?;
    let settings = jira_config::resolve(&stored, llm_provider_manifest::jira_preset());
    if settings.base_url.trim().is_empty() {
        return None;
    }
    Some(browse_url(&settings.base_url, key))
}

fn browse_url(base_url: &str, key: &str) -> String {
    format!("{}/browse/{key}", base_url.trim_end_matches('/'))
}

fn remember_issue_key(
    artifact_id: &str,
    spec: JiraTicketSpec,
    key: &str,
) -> Result<(), JiraError> {
    let content = ArtifactContent::JiraTicket(JiraTicketSpec {
        issue_key: key.to_string(),
        ..spec
    });
    artifacts::update_agent(artifact_id, None, content)
        .map(|_| ())
        .map_err(|e| JiraError::Artifact(e.to_string()))
}

/// The ticket's own links, as Web Links. Title falls back to the link type
/// (`GIT`, `FIGMA`), matching what hand-written tickets carry.
fn record_links(content: &ArtifactContent) -> Vec<JiraWebLink> {
    let ArtifactContent::JiraTicket(spec) = content else {
        return Vec::new();
    };
    spec.links
        .iter()
        .filter(|link| !link.url.trim().is_empty())
        .map(|link| {
            let title = link.title.trim();
            JiraWebLink {
                url: link.url.trim().to_string(),
                title: if title.is_empty() {
                    link.kind.trim().to_string()
                } else {
                    title.to_string()
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::artifact::TicketLink;

    #[test]
    fn browse_url_joins_with_exactly_one_slash() {
        assert_eq!(
            browse_url("https://jira.example.com/", "ABC-1"),
            "https://jira.example.com/browse/ABC-1"
        );
        assert_eq!(
            browse_url("https://jira.example.com", "ABC-1"),
            "https://jira.example.com/browse/ABC-1"
        );
    }

    #[test]
    fn links_without_a_url_are_not_attached() {
        let content = ArtifactContent::JiraTicket(JiraTicketSpec {
            links: vec![
                TicketLink { kind: "GIT".into(), url: "https://git/x".into(), title: String::new() },
                TicketLink { kind: "FIGMA".into(), url: "  ".into(), title: "Макет".into() },
            ],
            ..Default::default()
        });
        assert_eq!(
            record_links(&content),
            vec![JiraWebLink { url: "https://git/x".into(), title: "GIT".into() }]
        );
    }

    #[test]
    fn a_links_own_title_wins_over_its_type() {
        let content = ArtifactContent::JiraTicket(JiraTicketSpec {
            links: vec![TicketLink {
                kind: "GIT".into(),
                url: "https://git/x".into(),
                title: "документация".into(),
            }],
            ..Default::default()
        });
        assert_eq!(record_links(&content)[0].title, "документация");
    }
}
