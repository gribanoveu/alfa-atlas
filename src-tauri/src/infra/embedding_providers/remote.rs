//! OpenAI-compatible `/embeddings` endpoint — the same request/response
//! shape Together, Mistral, and local servers like Ollama/LM Studio also
//! speak, so this isn't locked to one vendor. Blocking HTTP (`ureq`), not
//! `reqwest`: this project's `tokio` dependency only enables
//! `sync, rt, macros, time` (no `net`), and a single blocking POST per
//! `embed` call doesn't justify expanding that.
//!
//! Uses a per-provider `ureq::Agent` (via `infra::http_agent`) so a
//! corporate internal CA from the bundled embedding preset — or a user
//! override — can replace the agent's trust store, same as the LLM client.
//!
//! Optional `request_headers` are sent on every POST (after `Authorization`).
//! Values of `$uuid` (see `domain::embeddings::REQUEST_HEADER_VALUE_UUID`)
//! are replaced with a fresh UUID per request.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::domain::embeddings::{Embedding, EmbeddingError, EmbeddingProvider, REQUEST_HEADER_VALUE_UUID};
use crate::infra::http_agent;

#[derive(Debug, Serialize)]
struct EmbeddingsRequest<'a> {
    input: &'a [&'a str],
    model: &'a str,
    encoding_format: &'static str,
}

#[derive(Debug, Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
}

pub struct RemoteEmbeddingProvider {
    agent: ureq::Agent,
    base_url: String,
    model: String,
    api_key: String,
    dimensions: usize,
    request_headers: HashMap<String, String>,
}

impl RemoteEmbeddingProvider {
    pub fn new(
        base_url: String,
        model: String,
        api_key: String,
        dimensions: usize,
        trusted_cert_pem: Option<&str>,
        request_headers: HashMap<String, String>,
        disable_tls_verification: bool,
    ) -> Result<Self, EmbeddingError> {
        let agent = http_agent::build_agent_with_options(trusted_cert_pem, disable_tls_verification)
            .map_err(|e| EmbeddingError::Tls(e.0))?;
        Ok(Self {
            agent,
            base_url,
            model,
            api_key,
            dimensions,
            request_headers,
        })
    }

    fn embeddings_url(&self) -> String {
        format!("{}/embeddings", self.base_url.trim_end_matches('/'))
    }
}

fn resolve_header_value(value: &str) -> String {
    if value == REQUEST_HEADER_VALUE_UUID {
        uuid::Uuid::new_v4().to_string()
    } else {
        value.to_string()
    }
}

impl EmbeddingProvider for RemoteEmbeddingProvider {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
        let body = EmbeddingsRequest {
            input: texts,
            model: &self.model,
            encoding_format: "float",
        };

        let mut request = self
            .agent
            .post(self.embeddings_url())
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("accept", "application/json");

        for (name, value) in &self.request_headers {
            request = request.header(name.as_str(), resolve_header_value(value));
        }

        let mut response = request
            .send_json(&body)
            .map_err(|e| EmbeddingError::Http(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let err_body = response
                .body_mut()
                .read_to_string()
                .unwrap_or_else(|e| format!("<failed to read error response body: {e}>"));
            return Err(EmbeddingError::Http(format!(
                "http status {status}: {err_body}"
            )));
        }

        let parsed: EmbeddingsResponse = response
            .body_mut()
            .read_json()
            .map_err(|e| EmbeddingError::Http(e.to_string()))?;

        Ok(parsed
            .data
            .into_iter()
            .map(|d| Embedding(d.embedding))
            .collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(base_url: &str) -> RemoteEmbeddingProvider {
        RemoteEmbeddingProvider::new(
            base_url.to_string(),
            "text-embedding-3-small".to_string(),
            "key".to_string(),
            1536,
            None,
            HashMap::new(),
            false,
        )
        .unwrap()
    }

    #[test]
    fn embeddings_url_strips_trailing_slash() {
        let p = provider("https://api.example.com/v1/");
        assert_eq!(p.embeddings_url(), "https://api.example.com/v1/embeddings");
    }

    #[test]
    fn parses_openai_compatible_response_shape() {
        let json = r#"{"data":[{"embedding":[0.1,0.2,0.3]},{"embedding":[0.4,0.5,0.6]}]}"#;
        let parsed: EmbeddingsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.data[0].embedding, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn request_serializes_with_input_model_and_encoding_format() {
        let texts = ["a", "b"];
        let req = EmbeddingsRequest {
            input: &texts,
            model: "text-embedding-3-small",
            encoding_format: "float",
        };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(
            json,
            r#"{"input":["a","b"],"model":"text-embedding-3-small","encoding_format":"float"}"#
        );
    }

    #[test]
    fn rejects_malformed_trusted_cert() {
        let result = RemoteEmbeddingProvider::new(
            "https://api.example.com/v1".to_string(),
            "m".to_string(),
            "key".to_string(),
            1536,
            Some("not a pem"),
            HashMap::new(),
            false,
        );
        assert!(matches!(result, Err(EmbeddingError::Tls(_))));
    }

    #[test]
    fn uuid_placeholder_expands_to_different_values() {
        let a = resolve_header_value(REQUEST_HEADER_VALUE_UUID);
        let b = resolve_header_value(REQUEST_HEADER_VALUE_UUID);
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
    }
}
