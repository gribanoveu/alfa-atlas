//! Resident LLM provider state and the resolution every LLM caller runs
//! before it can talk to a model.
//!
//! Split out of `commands::llm` so the application layer owns this rather
//! than the IPC boundary: `commands::memory_pipeline` needed the same slot
//! and helper and was reaching sideways into another command module for
//! them, and `lib.rs` was composing managed state out of `commands/`.
//!
//! The `settings -> resolved provider -> api key -> cached client -> model`
//! sequence was copy-pasted at six call sites before it landed here.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::domain::llm::{LlmProvider, LlmSettings, ResolvedLlmProvider};
use crate::infra::{llm_credentials_store, llm_providers};
use crate::services::llm_config;

/// Caches the constructed `LlmProvider` across calls, same reasoning as
/// `services::embedding_state::EmbeddingProviderSlot`: keyed by
/// `(resolved, api_key)` rather than just the provider id, so a key
/// rotation or a settings-layer override change (a different `base_url`/
/// `trusted_cert_pem`) invalidates the cache instead of silently reusing a
/// stale `ureq::Agent`.
pub type LlmProviderSlot = Mutex<Option<(ResolvedLlmProvider, Option<String>, Arc<dyn LlmProvider>)>>;

/// One flag for "the user asked the in-flight turn to stop" — this app has
/// exactly one chat panel / one in-flight conversation at a time (same
/// assumption `CHAT_STREAM_DELTA_EVENT` already makes), so a single
/// `Arc<AtomicBool>` needs no per-turn/per-request id to disambiguate.
/// `llm_chat_stream` resets this to `false` at the start of every *fresh*
/// turn (never `llm_chat_stream_resume`, which continues a turn already in
/// progress and must not lose a cancellation that landed while a
/// `PendingApproval` card was showing — see `llm_cancel_chat`'s doc
/// comment); `run_tool_loop` polls it at the checkpoints documented on its
/// own doc comment and resolves `ChatStreamOutcome::Cancelled` instead of
/// continuing once it reads `true`.
pub type ChatCancelFlag = AtomicBool;

pub fn ensure_provider(
    slot: &LlmProviderSlot,
    resolved: &ResolvedLlmProvider,
    api_key: Option<String>,
) -> Result<Arc<dyn LlmProvider>, String> {
    let mut guard = slot.lock().map_err(|_| "llm provider lock poisoned".to_string())?;
    let stale = !matches!(guard.as_ref(), Some((r, k, _)) if r == resolved && *k == api_key);
    if stale {
        let provider =
            llm_providers::provider_for(resolved, api_key.clone()).map_err(|e| e.to_string())?;
        *guard = Some((resolved.clone(), api_key, Arc::from(provider)));
    }
    Ok(guard.as_ref().expect("just set above if missing").2.clone())
}

/// Everything one LLM call needs, resolved once: the cached client, the id
/// it was resolved from (for rate-limit accounting and debug logging), the
/// model to send, and the settings that decided all three.
pub struct LlmSession {
    pub provider: Arc<dyn LlmProvider>,
    pub provider_id: String,
    pub model: String,
    pub settings: LlmSettings,
}

/// Settings -> merged provider view -> api key -> cached client, stopping
/// short of picking a model.
///
/// Separate from `resolve` on purpose, not for convenience: `llm_list_models`
/// has no model to send yet (it is what discovers them), and
/// `llm_test_connection` deliberately verifies the whole stack *without*
/// requiring a resolvable model — see its own doc comment. Folding these
/// into `resolve` would make both fail on exactly the misconfiguration they
/// exist to diagnose.
pub fn resolve_provider_only(
    provider_id: &str,
    slot: &LlmProviderSlot,
) -> Result<(Arc<dyn LlmProvider>, ResolvedLlmProvider, LlmSettings), String> {
    let settings = llm_config::load_llm_settings().map_err(|e| e.to_string())?;
    let resolved =
        llm_config::resolve_provider(provider_id, &settings).map_err(|e| e.to_string())?;
    let api_key = llm_credentials_store::get_api_key(provider_id);
    let provider = ensure_provider(slot, &resolved, api_key)?;
    Ok((provider, resolved, settings))
}

/// `resolve_provider_only` plus `llm_config::effective_model` — what every
/// caller that actually sends a completion needs.
pub fn resolve(provider_id: &str, slot: &LlmProviderSlot) -> Result<LlmSession, String> {
    let (provider, resolved, settings) = resolve_provider_only(provider_id, slot)?;
    let model =
        llm_config::effective_model(&resolved, provider.as_ref()).map_err(|e| e.to_string())?;
    Ok(LlmSession {
        provider,
        provider_id: provider_id.to_string(),
        model,
        settings,
    })
}
