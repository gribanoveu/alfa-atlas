pub mod openai_compatible;

use crate::domain::llm::{LlmError, LlmProvider, ResolvedLlmProvider};

/// Resolves a merged `ResolvedLlmProvider` (see `services::llm_config::
/// resolve_provider`) plus an API key already read from
/// `infra::llm_credentials_store` into a concrete `LlmProvider`. The one
/// place that decision is made — callers work against the trait
/// afterward, never against `OpenAiCompatibleProvider` directly. Today
/// always constructs an `OpenAiCompatibleProvider`, since every provider
/// (system or custom) speaks that protocol — see `domain::llm`'s module
/// doc for why there's no kind enum to branch on yet.
pub fn provider_for(
    resolved: &ResolvedLlmProvider,
    api_key: Option<String>,
) -> Result<Box<dyn LlmProvider>, LlmError> {
    let api_key = api_key.ok_or_else(|| {
        LlmError::Message(format!("no API key configured for provider \"{}\"", resolved.id))
    })?;
    let agent = openai_compatible::build_agent(resolved.trusted_cert_pem.as_deref())?;
    Ok(Box::new(openai_compatible::OpenAiCompatibleProvider::new(
        agent,
        resolved.base_url.clone(),
        api_key,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(trusted_cert_pem: Option<&str>) -> ResolvedLlmProvider {
        ResolvedLlmProvider {
            id: "alfagen".to_string(),
            label: "AlfaGen".to_string(),
            base_url: "https://example.internal".to_string(),
            is_system: true,
            model: None,
            trusted_cert_pem: trusted_cert_pem.map(|s| s.to_string()),
            limit: None,
        }
    }

    #[test]
    fn provider_for_without_api_key_errors_clearly() {
        let Err(err) = provider_for(&resolved(None), None) else {
            panic!("expected an error");
        };
        assert!(matches!(err, LlmError::Message(_)));
    }

    #[test]
    fn provider_for_with_a_malformed_trust_cert_errors_clearly() {
        let Err(err) = provider_for(&resolved(Some("not a pem")), Some("key".to_string())) else {
            panic!("expected an error");
        };
        assert!(matches!(err, LlmError::Tls(_)));
    }

    #[test]
    fn provider_for_succeeds_with_no_trust_cert_override() {
        assert!(provider_for(&resolved(None), Some("key".to_string())).is_ok());
    }
}
