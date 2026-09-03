//! Loads/saves `AppSettings.llm` and resolves a provider id (a system
//! preset plus its settings-layer override, or a standalone custom entry)
//! into the merged `ResolvedLlmProvider` view the rest of the app uses.
//! Pure, no I/O beyond `settings_store` — see `infra::llm_provider_manifest`
//! for where the compiled-in preset data itself comes from.

use std::collections::HashMap;

use crate::domain::llm::{
    DEFAULT_PROVIDER_TOKEN_LIMIT, LlmError, LlmProvider, LlmProviderConfig, LlmProviderPreset,
    LlmSettings, ResolvedLlmProvider,
};
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
        // A stored certificate byte-identical to the manifest's is treated
        // as *no* override: it carries no information, and an older build
        // could produce exactly that (the Settings form was seeded with the
        // resolved value, so "Сохранить сертификат" pinned the bundled PEM
        // as the user's own — after which a manifest update would never
        // reach that user again).
        let trusted_cert_override = over
            .and_then(|o| o.trusted_cert_pem.clone())
            .filter(|pem| Some(pem) != preset.trusted_cert_pem.as_ref());
        let trusted_cert_pem = trusted_cert_override
            .clone()
            .or_else(|| preset.trusted_cert_pem.clone());
        let limit = over.and_then(|o| o.limit).or(preset.limit);
        let known_models = over.map(|o| o.known_models.clone()).unwrap_or_default();
        let request_headers = resolve_request_headers(preset, over);
        let temperature = over.and_then(|o| o.temperature).or(preset.temperature);
        let max_tokens = over.and_then(|o| o.max_tokens).or(preset.max_tokens);
        let reasoning_effort = over
            .and_then(|o| o.reasoning_effort.clone())
            .or_else(|| preset.reasoning_effort.clone());
        return Ok(ResolvedLlmProvider {
            id: id.to_string(),
            label,
            base_url,
            is_system: true,
            model,
            trusted_cert_pem,
            trusted_cert_override,
            has_bundled_cert: preset.trusted_cert_pem.is_some(),
            known_models,
            limit,
            request_headers,
            temperature,
            max_tokens,
            reasoning_effort,
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
        // No manifest layer here, so merged and override are the same value.
        trusted_cert_pem: over.trusted_cert_pem.clone(),
        trusted_cert_override: over.trusted_cert_pem.clone(),
        has_bundled_cert: false,
        known_models: over.known_models.clone(),
        limit: over.limit.or(Some(DEFAULT_PROVIDER_TOKEN_LIMIT)),
        request_headers: resolve_request_headers_from_override(over),
        // No manifest layer to fall back on, and no way to know what this
        // provider's model accepts — unset stays unset. The same goes for
        // the two generation knobs below: an unknown gateway may reject a
        // request merely for carrying `reasoning_effort` at all.
        temperature: over.temperature,
        max_tokens: over.max_tokens,
        reasoning_effort: over.reasoning_effort.clone(),
    })
}

/// HTTP headers for LLM requests. Settings override replaces the preset
/// map entirely when set; otherwise the bundled preset applies.
pub fn resolve_request_headers(
    preset: &LlmProviderPreset,
    over: Option<&LlmProviderConfig>,
) -> HashMap<String, String> {
    if let Some(headers) = over.and_then(|o| o.request_headers.as_ref()) {
        return headers.clone();
    }
    preset.request_headers.clone().unwrap_or_default()
}

fn resolve_request_headers_from_override(over: &LlmProviderConfig) -> HashMap<String, String> {
    over.request_headers.clone().unwrap_or_default()
}

/// Every provider available for selection: one row per manifest preset
/// (always present, merged with its override if any — manifest order),
/// followed by settings-only (custom) ids in `settings.providers` order.
/// Skips an id that fails to resolve (e.g. a custom entry that's missing
/// its `base_url` so far) rather than surfacing a hard error for what's
/// often just a form the user hasn't finished filling in yet.
/// Provider id used for one-shot LLM callers (memory extraction, compaction, …).
/// Matches the frontend fallback: explicit `active_provider_id`, else the first
/// entry in [`list_resolved_providers`] (same order as the Settings picker).
pub fn effective_active_provider_id(settings: &LlmSettings) -> Option<String> {
    if let Some(id) = settings.active_provider_id.clone() {
        return Some(id);
    }
    list_resolved_providers(settings).into_iter().next().map(|p| p.id)
}

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
/// — no network call. When unpinned, fetches `/models` once, persists the
/// first result as a settings-layer override, and returns it — subsequent
/// calls reuse that pin until the user clears it (Settings → «Авто») or
/// removes the provider override.
pub fn effective_model(
    resolved: &ResolvedLlmProvider,
    provider: &dyn LlmProvider,
) -> Result<String, LlmError> {
    if let Some(model) = &resolved.model {
        return Ok(model.clone());
    }
    let model = provider
        .list_models()?
        .into_iter()
        .next()
        .map(|m| m.id)
        .ok_or_else(|| {
            LlmError::Provider(format!("provider \"{}\" returned no models", resolved.id))
        })?;
    pin_provider_model(&resolved.id, &model).map_err(|e| LlmError::Message(e.to_string()))?;
    Ok(model)
}

/// Writes `model` into the settings-layer override for `provider_id`,
/// preserving any other override fields already stored for that id.
pub fn pin_provider_model(provider_id: &str, model: &str) -> Result<(), SettingsError> {
    let mut settings = load_llm_settings()?;
    let existing = settings.providers.iter().find(|p| p.id == provider_id).cloned();
    let mut config = existing.unwrap_or_else(|| LlmProviderConfig {
        id: provider_id.to_string(),
        ..Default::default()
    });
    config.model = Some(model.to_string());
    upsert_provider_config(&mut settings, config);
    save_llm_settings(settings)
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
    use crate::domain::llm::{ChatRequest, ChatResponse, ChatStreamResult, LlmModelInfo, LlmProviderPreset, REQUEST_HEADER_VALUE_UUID};

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
            _on_tool_call_delta: &dyn Fn(&str, &str, &str),
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
    fn effective_active_provider_id_falls_back_to_first_resolved_provider() {
        let settings = LlmSettings {
            active_provider_id: None,
            providers: vec![LlmProviderConfig {
                id: "alfagen".to_string(),
                label: None,
                base_url: None,
                model: Some("DeepSeek-V4-Flash".to_string()),
                trusted_cert_pem: None,
                known_models: vec![],
                limit: None,
                request_headers: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            }],
            ..Default::default()
        };
        assert_eq!(
            effective_active_provider_id(&settings).as_deref(),
            Some("alfagen")
        );

        let pinned = LlmSettings {
            active_provider_id: Some("alfagen".to_string()),
            ..settings
        };
        assert_eq!(
            effective_active_provider_id(&pinned).as_deref(),
            Some("alfagen")
        );
    }

    #[test]
    fn resolve_system_provider_with_no_override_uses_manifest_values() {
        let settings = LlmSettings::default();
        let resolved = resolve_provider("alfagen", &settings).unwrap();
        assert_eq!(resolved.label, "AlfaGen");
        assert_eq!(resolved.base_url, "https://alfagen.moscow.alfaintra.net/continue-dev/v1");
        assert!(resolved.is_system);
        // AlfaGen ships without a manifest default model — the no-override
        // case resolves to `None`; `effective_model` fetches `/models` once
        // and persists the first result until the user changes it.
        assert_eq!(resolved.model, None);
        // AlfaGen ships with a real bundled trust cert (its internal CA
        // root) — see `infra::llm_provider_manifest`'s doc comment — so the
        // no-override case must still resolve to *that*, not `None`.
        assert!(resolved.trusted_cert_pem.unwrap().contains("BEGIN CERTIFICATE"));
        // …but the *override* stays empty, which is what the Settings form
        // binds to. Leaking the manifest PEM into that field would make it
        // indistinguishable from the user's own, and saving the form would
        // pin the build's default so later manifest changes stop arriving.
        assert_eq!(resolved.trusted_cert_override, None);
        assert!(resolved.has_bundled_cert);
        assert_eq!(
            resolved.limit,
            llm_provider_manifest::find_system_provider("alfagen")
                .and_then(|p| p.limit)
        );
    }

    /// Heals settings where the form, seeded with the resolved value, saved
    /// the manifest's own certificate back as a user override.
    #[test]
    fn a_stored_copy_of_the_manifest_certificate_is_not_an_override() {
        let bundled = llm_provider_manifest::find_system_provider("alfagen")
            .and_then(|p| p.trusted_cert_pem.clone())
            .expect("alfagen ships a bundled certificate");
        let settings = LlmSettings {
            providers: vec![LlmProviderConfig {
                id: "alfagen".to_string(),
                trusted_cert_pem: Some(bundled.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let resolved = resolve_provider("alfagen", &settings).unwrap();
        assert_eq!(resolved.trusted_cert_override, None);
        // The client still gets a certificate — only the form sees nothing.
        assert_eq!(resolved.trusted_cert_pem.as_deref(), Some(bundled.as_str()));
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
                known_models: vec![],
                limit: None,
                request_headers: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
            rate_limit_enabled: true,
            ..Default::default()
        };
        let resolved = resolve_provider("alfagen", &settings).unwrap();
        // Unset override fields still fall back to the manifest.
        assert_eq!(resolved.label, "AlfaGen");
        assert_eq!(resolved.base_url, "https://alfagen.moscow.alfaintra.net/continue-dev/v1");
        assert_eq!(resolved.limit, llm_provider_manifest::find_system_provider("alfagen").and_then(|p| p.limit));
        assert!(resolved.request_headers.is_empty());
        // Set override fields win.
        assert_eq!(resolved.model.as_deref(), Some("gpt-4o-mini"));
        assert!(resolved.trusted_cert_pem.unwrap().starts_with("-----BEGIN CERTIFICATE-----"));
        // A user certificate shows up in both: merged (it wins) and as the
        // override the form renders.
        assert_eq!(
            resolved.trusted_cert_override.as_deref(),
            Some("-----BEGIN CERTIFICATE-----\n...")
        );
        assert!(resolved.has_bundled_cert);
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
                known_models: vec![],
                limit: None,
                request_headers: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
            rate_limit_enabled: true,
            ..Default::default()
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
                known_models: vec![],
                limit: None,
                request_headers: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
            rate_limit_enabled: true,
            ..Default::default()
        };
        let resolved = resolve_provider("my-custom", &settings).unwrap();
        assert!(!resolved.is_system);
        assert_eq!(resolved.base_url, "https://api.openai.com/v1");
        assert_eq!(resolved.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn resolve_custom_provider_without_limit_gets_default() {
        let settings = LlmSettings {
            active_provider_id: None,
            providers: vec![LlmProviderConfig {
                id: "my-custom".to_string(),
                label: Some("My Custom".to_string()),
                base_url: Some("https://api.openai.com/v1".to_string()),
                model: None,
                trusted_cert_pem: None,
                known_models: vec![],
                limit: None,
                request_headers: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            }],
            ..Default::default()
        };
        let resolved = resolve_provider("my-custom", &settings).unwrap();
        assert_eq!(resolved.limit, Some(DEFAULT_PROVIDER_TOKEN_LIMIT));
    }

    #[test]
    fn resolve_provider_exposes_saved_model_catalog() {
        let settings = LlmSettings {
            active_provider_id: None,
            providers: vec![LlmProviderConfig {
                id: "openrouter".to_string(),
                label: Some("OpenRouter".to_string()),
                base_url: Some("https://openrouter.ai/api/v1".to_string()),
                model: Some("anthropic/claude-3.5-sonnet".to_string()),
                trusted_cert_pem: None,
                known_models: vec![
                    "anthropic/claude-3.5-sonnet".to_string(),
                    "openai/gpt-4o".to_string(),
                ],
                limit: None,
                request_headers: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            }],
            ..Default::default()
        };
        let resolved = resolve_provider("openrouter", &settings).unwrap();
        assert_eq!(resolved.known_models.len(), 2);
        assert_eq!(resolved.model.as_deref(), Some("anthropic/claude-3.5-sonnet"));
        let model = effective_model(
            &resolved,
            &FakeProvider {
                models: vec!["ignored"],
                panics_on_list: true,
            },
        )
        .unwrap();
        assert_eq!(model, "anthropic/claude-3.5-sonnet");
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
                known_models: vec![],
                limit: None,
                request_headers: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
            rate_limit_enabled: true,
            ..Default::default()
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
                known_models: vec![],
                limit: None,
                request_headers: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
            rate_limit_enabled: true,
            ..Default::default()
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
                known_models: vec![],
                limit: None,
                request_headers: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
            rate_limit_enabled: true,
            ..Default::default()
        };
        let list = list_resolved_providers(&settings);
        assert!(!list.iter().any(|p| p.id == "unfinished"));
    }

    #[test]
    fn pin_provider_model_preserves_manifest_provider_limit() {
        use crate::infra::settings_store::test_support::with_temp_home;

        with_temp_home(|| {
            pin_provider_model("alfagen", "other-model").unwrap();
            let settings = load_llm_settings().unwrap();
            let resolved = resolve_provider("alfagen", &settings).unwrap();
            assert_eq!(resolved.model.as_deref(), Some("other-model"));
            assert_eq!(
                resolved.limit,
                llm_provider_manifest::find_system_provider("alfagen").and_then(|p| p.limit)
            );
        });
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
            trusted_cert_override: None,
            has_bundled_cert: false,
            known_models: vec![],
            limit: None,
            request_headers: HashMap::new(),
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
        };
        let provider = FakeProvider { models: vec![], panics_on_list: true };
        let model = effective_model(&resolved, &provider).unwrap();
        assert_eq!(model, "pinned-model");
    }

    #[test]
    fn effective_model_pins_first_live_model_and_reuses_it() {
        use crate::infra::settings_store::test_support::with_temp_home;

        with_temp_home(|| {
            let resolved = ResolvedLlmProvider {
                id: "alfagen".to_string(),
                label: "AlfaGen".to_string(),
                base_url: "https://example.internal".to_string(),
                is_system: true,
                model: None,
                trusted_cert_pem: None,
                trusted_cert_override: None,
                has_bundled_cert: false,
                known_models: vec![],
                limit: None,
                request_headers: HashMap::new(),
                temperature: None,
                max_tokens: None,
                reasoning_effort: None,
            };
            let provider =
                FakeProvider { models: vec!["model-a", "model-b"], panics_on_list: false };
            assert_eq!(effective_model(&resolved, &provider).unwrap(), "model-a");

            let settings = load_llm_settings().unwrap();
            let entry = settings.providers.iter().find(|p| p.id == "alfagen").unwrap();
            assert_eq!(entry.model.as_deref(), Some("model-a"));

            let resolved = resolve_provider("alfagen", &settings).unwrap();
            assert_eq!(resolved.model.as_deref(), Some("model-a"));
            let provider = FakeProvider { models: vec![], panics_on_list: true };
            assert_eq!(effective_model(&resolved, &provider).unwrap(), "model-a");
        });
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
            trusted_cert_override: None,
            has_bundled_cert: false,
            known_models: vec![],
            limit: None,
            request_headers: HashMap::new(),
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
        };
        let provider = FakeProvider { models: vec![], panics_on_list: false };
        assert!(effective_model(&resolved, &provider).is_err());
    }

    #[test]
    fn upsert_provider_config_replaces_by_id_rather_than_duplicating() {
        let mut settings = LlmSettings::default();
        upsert_provider_config(
            &mut settings,
            LlmProviderConfig { id: "a".to_string(), label: Some("First".to_string()), base_url: None, model: None, trusted_cert_pem: None, known_models: vec![], limit: None, request_headers: None, temperature: None, max_tokens: None, reasoning_effort: None },
        );
        upsert_provider_config(
            &mut settings,
            LlmProviderConfig { id: "a".to_string(), label: Some("Second".to_string()), base_url: None, model: None, trusted_cert_pem: None, known_models: vec![], limit: None, request_headers: None, temperature: None, max_tokens: None, reasoning_effort: None },
        );
        assert_eq!(settings.providers.len(), 1);
        assert_eq!(settings.providers[0].label.as_deref(), Some("Second"));
    }

    #[test]
    fn upsert_provider_config_appends_a_new_id() {
        let mut settings = LlmSettings::default();
        upsert_provider_config(
            &mut settings,
            LlmProviderConfig { id: "a".to_string(), label: None, base_url: None, model: None, trusted_cert_pem: None, known_models: vec![], limit: None, request_headers: None, temperature: None, max_tokens: None, reasoning_effort: None },
        );
        upsert_provider_config(
            &mut settings,
            LlmProviderConfig { id: "b".to_string(), label: None, base_url: None, model: None, trusted_cert_pem: None, known_models: vec![], limit: None, request_headers: None, temperature: None, max_tokens: None, reasoning_effort: None },
        );
        assert_eq!(settings.providers.len(), 2);
    }

    #[test]
    fn remove_provider_config_clears_active_id_when_it_matches() {
        let mut settings = LlmSettings {
            active_provider_id: Some("a".to_string()),
            providers: vec![LlmProviderConfig { id: "a".to_string(), label: None, base_url: None, model: None, trusted_cert_pem: None, known_models: vec![], limit: None, request_headers: None, temperature: None, max_tokens: None, reasoning_effort: None }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
            rate_limit_enabled: true,
            ..Default::default()
        };
        remove_provider_config(&mut settings, "a");
        assert!(settings.providers.is_empty());
        assert_eq!(settings.active_provider_id, None);
    }

    #[test]
    fn remove_provider_config_leaves_active_id_alone_when_it_does_not_match() {
        let mut settings = LlmSettings {
            active_provider_id: Some("b".to_string()),
            providers: vec![LlmProviderConfig { id: "a".to_string(), label: None, base_url: None, model: None, trusted_cert_pem: None, known_models: vec![], limit: None, request_headers: None, temperature: None, max_tokens: None, reasoning_effort: None }],
            debug_logging: false,
            follow_up_suggestions_disabled: false,
            tool_call_logging: true,
            task_done_sound_enabled: true,
            need_answer_sound_enabled: true,
            rate_limit_enabled: true,
            ..Default::default()
        };
        remove_provider_config(&mut settings, "a");
        assert_eq!(settings.active_provider_id.as_deref(), Some("b"));
    }

    #[test]
    fn settings_request_headers_override_replaces_preset() {
        let preset = llm_provider_manifest::find_system_provider("alfagen").expect("alfagen");
        let mut preset_with_headers = preset.clone();
        preset_with_headers.request_headers = Some(HashMap::from([
            ("systemId".into(), "preset".into()),
            ("messageId".into(), REQUEST_HEADER_VALUE_UUID.into()),
        ]));

        let settings = LlmSettings {
            providers: vec![LlmProviderConfig {
                id: "alfagen".to_string(),
                request_headers: Some(HashMap::from([("X-Custom".into(), "1".into())])),
                ..Default::default()
            }],
            ..Default::default()
        };

        let headers = resolve_request_headers(&preset_with_headers, settings.providers.first());
        assert_eq!(headers.get("X-Custom").map(String::as_str), Some("1"));
        assert!(!headers.contains_key("systemId"));
    }

    #[test]
    fn a_settings_temperature_overrides_the_manifest_preset() {
        // Same "override wins when Some" merge as limit/requestHeaders; the
        // manifest ships a value, the user is allowed to disagree with it.
        let settings = LlmSettings {
            providers: vec![LlmProviderConfig {
                id: "alfagen".to_string(),
                label: None,
                base_url: None,
                model: None,
                trusted_cert_pem: None,
                known_models: vec![],
                limit: None,
                request_headers: None,
                temperature: Some(0.9),
                max_tokens: None,
                reasoning_effort: None,
            }],
            ..LlmSettings::default()
        };
        let resolved = resolve_provider("alfagen", &settings).unwrap();
        assert_eq!(resolved.temperature, Some(0.9));
    }

    #[test]
    fn a_custom_provider_sends_no_temperature_unless_told_to() {
        // Nothing knows what a hand-added endpoint's model accepts, and a
        // reasoning model rejects any temperature but its own.
        let settings = LlmSettings {
            providers: vec![LlmProviderConfig {
                id: "mine".to_string(),
                label: None,
                base_url: Some("https://example.com/v1".to_string()),
                model: None,
                trusted_cert_pem: None,
                known_models: vec![],
                limit: None,
                request_headers: None,
                temperature: None,
                max_tokens: None,
                reasoning_effort: None,
            }],
            ..LlmSettings::default()
        };
        let resolved = resolve_provider("mine", &settings).unwrap();
        assert_eq!(resolved.temperature, None);
    }

    #[test]
    fn preset_request_headers_apply_when_no_override() {
        let preset = LlmProviderPreset {
            id: "x".into(),
            label: "X".into(),
            base_url: "https://example.com/v1".into(),
            default_model: None,
            trusted_cert_pem: None,
            limit: None,
            request_headers: Some(HashMap::from([("systemId".into(), "sanduser".into())])),
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
        };
        let headers = resolve_request_headers(&preset, None);
        assert_eq!(headers.get("systemId").map(String::as_str), Some("sanduser"));
    }
}
