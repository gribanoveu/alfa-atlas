//! Loads/saves `AppSettings.llm` and resolves a provider id (system preset
//! + settings-layer override, or a standalone custom entry) into the
//! merged `ResolvedLlmProvider` view the rest of the app actually uses.
//! Pure, no I/O beyond `settings_store` — see `infra::llm_provider_manifest`
//! for where the compiled-in preset data itself comes from.

use crate::domain::llm::{LlmError, LlmProvider, LlmProviderConfig, LlmSettings, ResolvedLlmProvider};
use crate::domain::settings::SettingsError;
use crate::infra::llm_provider_manifest;
use crate::infra::settings_store;

pub fn load_llm_settings() -> Result<LlmSettings, SettingsError> {
    Ok(settings_store::load()?.llm)
}

pub fn save_llm_settings(settings: LlmSettings) -> Result<(), SettingsError> {
    let mut app = settings_store::load().unwrap_or_default();
    app.llm = settings;
    settings_store::save(&app)
}

/// Merges `id`'s manifest preset (if any) with its settings-layer override
/// (if any) — override field wins whenever `Some`, per
/// `LlmProviderConfig`'s doc comment. An id with no matching preset is
/// resolved entirely from the settings-layer entry (a custom provider);
/// one with neither a preset nor a settings entry is unknown.
pub fn resolve_provider(
    id: &str,
    settings: &LlmSettings,
) -> Result<ResolvedLlmProvider, LlmError> {
    let preset = llm_provider_manifest::find_system_provider(id);
    let over = settings.providers.iter().find(|p| p.id == id);

    if let Some(preset) = preset {
        let label = over
            .and_then(|o| o.label.clone())
            .unwrap_or_else(|| preset.label.clone());
        let base_url = over
            .and_then(|o| o.base_url.clone())
            .unwrap_or_else(|| preset.base_url.clone());
        let model = over
            .and_then(|o| o.model.clone())
            .or_else(|| preset.default_model.clone());
        let trusted_cert_pem = over
            .and_then(|o| o.trusted_cert_pem.clone())
            .or_else(|| preset.trusted_cert_pem.clone());
        let limit = over.and_then(|o| o.limit).or(preset.limit);
        return Ok(ResolvedLlmProvider {
            id: id.to_string(),
            label,
            base_url,
            is_system: true,
            model,
            trusted_cert_pem,
            limit,
        });
    }

    let over = over.ok_or_else(|| {
        LlmError::Message(format!("no LLM provider configured with id \"{id}\""))
    })?;
    let base_url = over.base_url.clone().ok_or_else(|| {
        LlmError::Message(format!("provider \"{id}\" has no base URL configured"))
    })?;
    Ok(ResolvedLlmProvider {
        id: id.to_string(),
        label: over.label.clone().unwrap_or_else(|| id.to_string()),
        base_url,
        is_system: false,
        model: over.model.clone(),
        trusted_cert_pem: over.trusted_cert_pem.clone(),
        limit: over.limit,
    })
}

/// Every provider available for selection: one row per manifest preset
/// (always present, merged with its override if any — manifest order),
/// followed by settings-only (custom) ids in `settings.providers` order.
/// Skips an id that fails to resolve (e.g. a custom entry that's missing
/// its `base_url` so far) rather than surfacing a hard error for what's
/// often just a form the user hasn't finished filling in yet.
pub fn list_resolved_providers(settings: &LlmSettings) -> Vec<ResolvedLlmProvider> {
    let mut out = Vec::new();
    for preset in llm_provider_manifest::system_providers() {
        if let Ok(resolved) = resolve_provider(&preset.id, settings) {
            out.push(resolved);
        }
    }
    for config in &settings.providers {
        if llm_provider_manifest::find_system_provider(&config.id).is_some() {
            continue; // already covered above
        }
        if let Ok(resolved) = resolve_provider(&config.id, settings) {
            out.push(resolved);
        }
    }
    out
}

/// The model a `ChatRequest` should actually use: the resolved pin
/// (explicit override, or the manifest's `default_model`) when one exists
/// — no network call — otherwise the first result from the provider's live
/// `list_models()`.
pub fn effective_model(
    resolved: &ResolvedLlmProvider,
    provider: &dyn LlmProvider,
) -> Result<String, LlmError> {
    if let Some(model) = &resolved.model {
        return Ok(model.clone());
    }
    provider
        .list_models()?
        .into_iter()
        .next()
        .map(|m| m.id)
        .ok_or_else(|| {
            LlmError::Provider(format!("provider \"{}\" returned no models", resolved.id))
        })
}

/// Replaces `config.id`'s existing entry, or appends a new one. Pure — kept
/// separate from `commands::llm` so it's directly unit-testable without any
/// IPC/mutex plumbing.
pub fn upsert_provider_config(settings: &mut LlmSettings, config: LlmProviderConfig) {
    if let Some(existing) = settings.providers.iter_mut().find(|p| p.id == config.id) {
        *existing = config;
    } else {
        settings.providers.push(config);
    }
}

/// Drops `provider_id`'s settings-layer entry (a system provider's manifest
/// preset is untouched — see `infra::llm_provider_manifest`'s doc comment
/// on why removal there is a rebuild, not a runtime, operation) and clears
/// `active_provider_id` if it pointed at the removed id.
pub fn remove_provider_config(settings: &mut LlmSettings, provider_id: &str) {
    settings.providers.retain(|p| p.id != provider_id);
    if settings.active_provider_id.as_deref() == Some(provider_id) {
        settings.active_provider_id = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::llm::{ChatRequest, ChatResponse, ChatStreamResult, LlmModelInfo, ModelLimit};

    struct FakeProvider {
        models: Vec<&'static str>,
        panics_on_list: bool,
    }

    impl LlmProvider for FakeProvider {
        fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
            unimplemented!("not exercised by these tests")
        }

        fn chat_stream(
            &self,
            _request: ChatRequest,
            _on_delta: &dyn Fn(&str),
            _on_reasoning: &dyn Fn(&str),
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<ChatStreamResult, LlmError> {
            unimplemented!("not exercised by these tests")
        }

        fn list_models(&self) -> Result<Vec<LlmModelInfo>, LlmError> {
            if self.panics_on_list {
                panic!("list_models should not have been called");
            }
            Ok(self.models.iter().map(|id| LlmModelInfo { id: id.to_string() }).collect())
        }
    }

    #[test]
    fn resolve_system_provider_with_no_override_uses_manifest_values() {
        let settings = LlmSettings::default();
        let resolved = resolve_provider("alfagen", &settings).unwrap();
        assert_eq!(resolved.label, "AlfaGen");
        assert_eq!(resolved.base_url, "https://alfagen.moscow.alfaintra.net/continue-dev/v1");
        assert!(resolved.is_system);
        // AlfaGen ships with a pinned manifest default model (see
        // `system_providers.json`) so the no-override case resolves to
        // that, not `None`.
        assert_eq!(resolved.model.as_deref(), Some("DeepSeek-V4-Flash"));
        // AlfaGen ships with a real bundled trust cert (its internal CA
        // root) — see `infra::llm_provider_manifest`'s doc comment — so the
        // no-override case must still resolve to *that*, not `None`.
        assert!(resolved.trusted_cert_pem.unwrap().contains("BEGIN CERTIFICATE"));
        assert_eq!(resolved.limit, Some(ModelLimit { context: 1_000_000, output: 30_000 }));
    }

    #[test]
    fn resolve_system_provider_override_wins_per_field() {
        let settings = LlmSettings {
            active_provider_id: None,
            providers: vec![LlmProviderConfig {
                id: "alfagen".to_string(),
                label: None,
                base_url: None,
                model: Some("gpt-4o-mini".to_string()),
                trusted_cert_pem: Some("-----BEGIN CERTIFICATE-----\n...".to_string()),
                limit: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
        };
        let resolved = resolve_provider("alfagen", &settings).unwrap();
        // Unset override fields still fall back to the manifest.
        assert_eq!(resolved.label, "AlfaGen");
        assert_eq!(resolved.base_url, "https://alfagen.moscow.alfaintra.net/continue-dev/v1");
        assert_eq!(resolved.limit, Some(ModelLimit { context: 1_000_000, output: 30_000 }));
        // Set override fields win.
        assert_eq!(resolved.model.as_deref(), Some("gpt-4o-mini"));
        assert!(resolved.trusted_cert_pem.unwrap().starts_with("-----BEGIN CERTIFICATE-----"));
    }

    #[test]
    fn resolve_unknown_id_is_an_error() {
        let settings = LlmSettings::default();
        assert!(resolve_provider("does-not-exist", &settings).is_err());
    }

    #[test]
    fn resolve_custom_provider_without_base_url_is_an_error() {
        let settings = LlmSettings {
            active_provider_id: None,
            providers: vec![LlmProviderConfig {
                id: "my-custom".to_string(),
                label: Some("My Custom".to_string()),
                base_url: None,
                model: None,
                trusted_cert_pem: None,
                limit: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
        };
        assert!(resolve_provider("my-custom", &settings).is_err());
    }

    #[test]
    fn resolve_custom_provider_with_base_url_succeeds() {
        let settings = LlmSettings {
            active_provider_id: None,
            providers: vec![LlmProviderConfig {
                id: "my-custom".to_string(),
                label: Some("My Custom".to_string()),
                base_url: Some("https://api.openai.com/v1".to_string()),
                model: Some("gpt-4o".to_string()),
                trusted_cert_pem: None,
                limit: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
        };
        let resolved = resolve_provider("my-custom", &settings).unwrap();
        assert!(!resolved.is_system);
        assert_eq!(resolved.base_url, "https://api.openai.com/v1");
        assert_eq!(resolved.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn list_resolved_providers_always_includes_every_manifest_preset_once() {
        let settings = LlmSettings {
            active_provider_id: None,
            providers: vec![LlmProviderConfig {
                id: "alfagen".to_string(),
                label: None,
                base_url: None,
                model: Some("pinned-model".to_string()),
                trusted_cert_pem: None,
                limit: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
        };
        let list = list_resolved_providers(&settings);
        let alfagen_rows: Vec<_> = list.iter().filter(|p| p.id == "alfagen").collect();
        assert_eq!(alfagen_rows.len(), 1, "a system id with an override must still be one row");
        assert_eq!(alfagen_rows[0].model.as_deref(), Some("pinned-model"));
    }

    #[test]
    fn list_resolved_providers_appends_custom_ids_after_manifest_ids() {
        let settings = LlmSettings {
            active_provider_id: None,
            providers: vec![LlmProviderConfig {
                id: "my-custom".to_string(),
                label: Some("Custom".to_string()),
                base_url: Some("https://example.com/v1".to_string()),
                model: None,
                trusted_cert_pem: None,
                limit: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
        };
        let list = list_resolved_providers(&settings);
        let ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["alfagen", "my-custom"]);
    }

    #[test]
    fn list_resolved_providers_skips_an_unfinished_custom_entry() {
        let settings = LlmSettings {
            active_provider_id: None,
            providers: vec![LlmProviderConfig {
                id: "unfinished".to_string(),
                label: None,
                base_url: None,
                model: None,
                trusted_cert_pem: None,
                limit: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
        };
        let list = list_resolved_providers(&settings);
        assert!(!list.iter().any(|p| p.id == "unfinished"));
    }

    #[test]
    fn effective_model_skips_list_models_when_pinned() {
        let resolved = ResolvedLlmProvider {
            id: "alfagen".to_string(),
            label: "AlfaGen".to_string(),
            base_url: "https://example.internal".to_string(),
            is_system: true,
            model: Some("pinned-model".to_string()),
            trusted_cert_pem: None,
            limit: None,
        };
        let provider = FakeProvider { models: vec![], panics_on_list: true };
        let model = effective_model(&resolved, &provider).unwrap();
        assert_eq!(model, "pinned-model");
    }

    #[test]
    fn effective_model_fetches_and_takes_the_first_live_model_when_unpinned() {
        let resolved = ResolvedLlmProvider {
            id: "alfagen".to_string(),
            label: "AlfaGen".to_string(),
            base_url: "https://example.internal".to_string(),
            is_system: true,
            model: None,
            trusted_cert_pem: None,
            limit: None,
        };
        let provider = FakeProvider { models: vec!["model-a", "model-b"], panics_on_list: false };
        let model = effective_model(&resolved, &provider).unwrap();
        assert_eq!(model, "model-a");
    }

    #[test]
    fn effective_model_errors_clearly_on_an_empty_live_list() {
        let resolved = ResolvedLlmProvider {
            id: "alfagen".to_string(),
            label: "AlfaGen".to_string(),
            base_url: "https://example.internal".to_string(),
            is_system: true,
            model: None,
            trusted_cert_pem: None,
            limit: None,
        };
        let provider = FakeProvider { models: vec![], panics_on_list: false };
        assert!(effective_model(&resolved, &provider).is_err());
    }

    #[test]
    fn upsert_provider_config_replaces_by_id_rather_than_duplicating() {
        let mut settings = LlmSettings::default();
        upsert_provider_config(
            &mut settings,
            LlmProviderConfig { id: "a".to_string(), label: Some("First".to_string()), base_url: None, model: None, trusted_cert_pem: None, limit: None },
        );
        upsert_provider_config(
            &mut settings,
            LlmProviderConfig { id: "a".to_string(), label: Some("Second".to_string()), base_url: None, model: None, trusted_cert_pem: None, limit: None },
        );
        assert_eq!(settings.providers.len(), 1);
        assert_eq!(settings.providers[0].label.as_deref(), Some("Second"));
    }

    #[test]
    fn upsert_provider_config_appends_a_new_id() {
        let mut settings = LlmSettings::default();
        upsert_provider_config(
            &mut settings,
            LlmProviderConfig { id: "a".to_string(), label: None, base_url: None, model: None, trusted_cert_pem: None, limit: None },
        );
        upsert_provider_config(
            &mut settings,
            LlmProviderConfig { id: "b".to_string(), label: None, base_url: None, model: None, trusted_cert_pem: None, limit: None },
        );
        assert_eq!(settings.providers.len(), 2);
    }

    #[test]
    fn remove_provider_config_clears_active_id_when_it_matches() {
        let mut settings = LlmSettings {
            active_provider_id: Some("a".to_string()),
            providers: vec![LlmProviderConfig { id: "a".to_string(), label: None, base_url: None, model: None, trusted_cert_pem: None, limit: None }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
        };
        remove_provider_config(&mut settings, "a");
        assert!(settings.providers.is_empty());
        assert_eq!(settings.active_provider_id, None);
    }

    #[test]
    fn remove_provider_config_leaves_active_id_alone_when_it_does_not_match() {
        let mut settings = LlmSettings {
            active_provider_id: Some("b".to_string()),
            providers: vec![LlmProviderConfig { id: "a".to_string(), label: None, base_url: None, model: None, trusted_cert_pem: None, limit: None }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
        };
        remove_provider_config(&mut settings, "a");
        assert_eq!(settings.active_provider_id.as_deref(), Some("b"));
    }
}
