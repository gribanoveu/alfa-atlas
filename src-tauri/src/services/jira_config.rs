//! Jira settings: the two-layer merge (build manifest under the user's own
//! settings) plus the one use case built on top of it — resolve settings +
//! stored token into a client and ask Jira who the token belongs to.
//!
//! Load/save mirror `services::spellcheck_prefs` (a section of
//! `~/.atlas/settings.json`); the merge mirrors
//! `services::llm_config::resolve_provider` (a manifest preset folded with
//! a settings-layer override). The token never passes through here as data,
//! only as something fetched from `infra::jira_credentials_store` at the
//! moment a request is made.

use crate::domain::jira::{
    JiraError, JiraIssueType, JiraLinkOutcome, JiraPreset, JiraProject, JiraSettings,
    JiraSettingsView, JiraUser, JiraWebLink,
};
use crate::domain::settings::SettingsError;
use crate::infra::{
    jira_client, jira_credentials_store, llm_provider_manifest, settings_store,
};

/// The user layer alone — what the settings form edits.
pub fn load_jira_settings() -> Result<JiraSettings, SettingsError> {
    Ok(settings_store::load()?.jira)
}

/// The user layer plus what the build would fall back to, so the settings
/// tab can say "задаётся сборкой" instead of showing an empty field that
/// nonetheless works.
pub fn load_jira_settings_view() -> Result<JiraSettingsView, SettingsError> {
    let preset = llm_provider_manifest::jira_preset();
    Ok(JiraSettingsView {
        settings: load_jira_settings()?,
        bundled_base_url: preset.base_url.as_deref().and_then(non_empty),
        has_bundled_cert: preset
            .trusted_cert_pem
            .as_deref()
            .and_then(non_empty)
            .is_some(),
    })
}

/// Normalizes before writing so everything downstream can assume trimmed
/// values, and a field someone cleared to whitespace reads back as "no
/// override" (falling back to the build preset) rather than as an empty
/// string that would shadow it.
pub fn save_jira_settings(settings: JiraSettings) -> Result<(), SettingsError> {
    let mut all = settings_store::load().unwrap_or_default();
    let stored = all.jira.clone();
    let project_key = settings.project_key.trim().to_uppercase();

    // Issue types are configured per project, so a type carried over from a
    // different project would name something the new one may not have. It is
    // dropped only when the caller did *not* supply a new one — picking a
    // project and its type in a single write must not erase the type.
    let project_changed = project_key != stored.project_key;
    let kept_old_type = settings.issue_type_id == stored.issue_type_id;
    let (issue_type_id, issue_type_name) = if project_changed && kept_old_type {
        (String::new(), String::new())
    } else {
        (
            settings.issue_type_id.trim().to_string(),
            settings.issue_type_name.trim().to_string(),
        )
    };

    all.jira = JiraSettings {
        issue_type_id,
        issue_type_name,
        base_url: settings.base_url.trim().trim_end_matches('/').to_string(),
        project_key,
        project_name: settings.project_name.trim().to_string(),
        trusted_cert_pem: settings
            .trusted_cert_pem
            .as_deref()
            .and_then(non_empty),
    };
    settings_store::save(&all)
}

/// The effective connection: each user field, or the build's default when
/// that field is empty. Same precedence as `llm_config::resolve_provider` —
/// override wins, preset fills the gap.
pub fn resolve(settings: &JiraSettings, preset: &JiraPreset) -> JiraSettings {
    JiraSettings {
        base_url: non_empty(&settings.base_url)
            .or_else(|| preset.base_url.as_deref().and_then(non_empty))
            .unwrap_or_default(),
        // No manifest counterpart: which project someone files tickets in,
        // and with which type, is a personal choice rather than something a
        // build can preset.
        project_key: settings.project_key.clone(),
        project_name: settings.project_name.clone(),
        issue_type_id: settings.issue_type_id.clone(),
        issue_type_name: settings.issue_type_name.clone(),
        trusted_cert_pem: settings
            .trusted_cert_pem
            .as_deref()
            .and_then(non_empty)
            .or_else(|| preset.trusted_cert_pem.as_deref().and_then(non_empty)),
    }
}

/// The account behind the stored token — both the right-dock panel's content
/// and its connection check, since there is nothing to show unless the round
/// trip succeeded. Blocking; callers run it on a blocking thread.
/// Settings + stored token → a connected client. Every call that talks to
/// Jira needs the identical four steps, and repeating them was already
/// three copies deep.
pub(crate) fn connect_stored() -> Result<gouqi::Jira, JiraError> {
    let stored = load_jira_settings().map_err(|e| JiraError::Settings(e.to_string()))?;
    let settings = resolve(&stored, llm_provider_manifest::jira_preset());
    if !settings.is_addressable() {
        return Err(JiraError::NotConfigured);
    }
    let token = jira_credentials_store::get_token().ok_or(JiraError::MissingToken)?;
    jira_client::connect(&settings, token)
}

pub fn current_user() -> Result<JiraUser, JiraError> {
    jira_client::current_user(&connect_stored()?)
}

/// The projects the stored token can see — the recent handful by default,
/// the full list when the user is searching. Blocking; callers run it on a
/// blocking thread.
pub fn list_projects(recent_only: bool) -> Result<Vec<JiraProject>, JiraError> {
    jira_client::list_projects(&connect_stored()?, recent_only)
}

/// The issue types `project_key` accepts, sub-tasks excluded. Blocking;
/// callers run it on a blocking thread.
pub fn list_issue_types(project_key: &str) -> Result<Vec<JiraIssueType>, JiraError> {
    let jira = connect_stored()?;
    jira_client::list_issue_types(&jira, project_key)
}

/// Attaches every link to `issue_key` as a Jira Web Link.
///
/// Never stops at the first failure: one bad URL must not silently drop the
/// links after it, and the caller shows the user exactly which ones landed.
/// Blocking; callers run it on a blocking thread.
pub fn attach_web_links(
    issue_key: &str,
    links: &[JiraWebLink],
) -> Result<Vec<JiraLinkOutcome>, JiraError> {
    let issue_key = issue_key.trim();
    if issue_key.is_empty() {
        return Err(JiraError::MissingIssueKey);
    }

    let jira = connect_stored()?;

    Ok(links
        .iter()
        .filter(|link| !link.url.trim().is_empty())
        .map(|link| JiraLinkOutcome {
            url: link.url.trim().to_string(),
            error: jira_client::attach_web_link(&jira, issue_key, link)
                .err()
                .map(|e| e.to_string()),
        })
        .collect())
}

/// Trimmed, or `None` when blank — the single rule for "this field carries
/// no value", applied to both layers.
fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;

    fn preset() -> JiraPreset {
        JiraPreset {
            base_url: Some("https://jira.build.example".to_string()),
            trusted_cert_pem: Some("BUILD PEM".to_string()),
        }
    }

    #[test]
    fn saving_trims_the_base_url_and_drops_its_trailing_slash() {
        with_temp_home(|| {
            save_jira_settings(JiraSettings {
                base_url: "  https://jira.example.com/  ".to_string(),
                trusted_cert_pem: Some("   ".to_string()),
                ..Default::default()
            })
            .unwrap();

            let loaded = load_jira_settings().unwrap();
            assert_eq!(loaded.base_url, "https://jira.example.com");
            // Whitespace is not an override — the build preset stays in play.
            assert_eq!(loaded.trusted_cert_pem, None);
        });
    }

    /// Jira project keys are upper-case; one typed in the wrong case would
    /// address nothing, and the picker is not the only way in — the field
    /// accepts typing too.
    #[test]
    fn the_remembered_project_key_is_upper_cased() {
        with_temp_home(|| {
            save_jira_settings(JiraSettings {
                base_url: "https://jira.example.com".to_string(),
                project_key: "  wowtax ".to_string(),
                project_name: "  Бухгалтерия  ".to_string(),
                ..Default::default()
            })
            .unwrap();

            let loaded = load_jira_settings().unwrap();
            assert_eq!(loaded.project_key, "WOWTAX");
            assert_eq!(loaded.project_name, "Бухгалтерия");
        });
    }

    /// The manifest has no say here: which project someone files tickets in
    /// is personal, so an empty choice stays empty rather than inheriting.
    #[test]
    fn the_project_choice_has_no_build_default() {
        let resolved = resolve(&JiraSettings::default(), &preset());
        assert_eq!(resolved.project_key, "");
        assert_eq!(resolved.issue_type_id, "");
    }

    /// Issue types are configured per project, so one carried over from
    /// another project would name something the new one may not have.
    #[test]
    fn changing_the_project_forgets_its_issue_type() {
        with_temp_home(|| {
            save_jira_settings(JiraSettings {
                base_url: "https://jira.example.com".to_string(),
                project_key: "ALPHA".to_string(),
                issue_type_id: "20".to_string(),
                issue_type_name: "User Story".to_string(),
                ..Default::default()
            })
            .unwrap();

            // The project picker writes only the project — it has no type to
            // offer for a project whose types it has not fetched.
            let mut next = load_jira_settings().unwrap();
            next.project_key = "BETA".to_string();
            save_jira_settings(next).unwrap();

            let loaded = load_jira_settings().unwrap();
            assert_eq!(loaded.project_key, "BETA");
            assert_eq!(loaded.issue_type_id, "");
            assert_eq!(loaded.issue_type_name, "");
        });
    }

    /// …but picking a project *and* its type in one write must not erase the
    /// type that write just supplied.
    #[test]
    fn a_project_and_type_chosen_together_both_survive() {
        with_temp_home(|| {
            save_jira_settings(JiraSettings {
                base_url: "https://jira.example.com".to_string(),
                project_key: "ALPHA".to_string(),
                issue_type_id: "20".to_string(),
                ..Default::default()
            })
            .unwrap();

            save_jira_settings(JiraSettings {
                base_url: "https://jira.example.com".to_string(),
                project_key: "BETA".to_string(),
                issue_type_id: "3".to_string(),
                issue_type_name: "Task".to_string(),
                ..Default::default()
            })
            .unwrap();

            let loaded = load_jira_settings().unwrap();
            assert_eq!(loaded.project_key, "BETA");
            assert_eq!(loaded.issue_type_id, "3");
        });
    }

    #[test]
    fn the_build_preset_fills_fields_the_user_left_empty() {
        let resolved = resolve(&JiraSettings::default(), &preset());
        assert_eq!(resolved.base_url, "https://jira.build.example");
        assert_eq!(resolved.trusted_cert_pem.as_deref(), Some("BUILD PEM"));
    }

    #[test]
    fn user_values_win_over_the_build_preset() {
        let resolved = resolve(
            &JiraSettings {
                base_url: "https://jira.mine.example".to_string(),
                trusted_cert_pem: Some("MY PEM".to_string()),
                ..Default::default()
            },
            &preset(),
        );
        assert_eq!(resolved.base_url, "https://jira.mine.example");
        assert_eq!(resolved.trusted_cert_pem.as_deref(), Some("MY PEM"));
    }

    #[test]
    fn an_empty_preset_leaves_an_unconfigured_instance_unconfigured() {
        let resolved = resolve(&JiraSettings::default(), &JiraPreset::default());
        assert!(!resolved.is_addressable());
        assert_eq!(resolved.trusted_cert_pem, None);
    }

    #[test]
    fn an_unconfigured_instance_never_reaches_the_network() {
        with_temp_home(|| {
            // Guards the manifest-shipped case too: if this build ever
            // starts shipping a `jira.baseUrl`, the assertion below moves to
            // `MissingToken`, which is still a pre-network refusal.
            let err = current_user().unwrap_err();
            assert!(
                matches!(err, JiraError::NotConfigured | JiraError::MissingToken),
                "unexpected error: {err}"
            );
        });
    }

    #[test]
    fn a_configured_instance_without_a_token_asks_for_one() {
        with_temp_home(|| {
            save_jira_settings(JiraSettings {
                base_url: "https://jira.example.com".to_string(),
                ..Default::default()
            })
            .unwrap();

            let err = current_user().unwrap_err();
            assert!(matches!(err, JiraError::MissingToken));
        });
    }
}
