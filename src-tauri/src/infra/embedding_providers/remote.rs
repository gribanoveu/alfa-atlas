//! OpenAI-compatible `/embeddings` endpoint — the same request/response
//! shape Together, Mistral, and local servers like Ollama/LM Studio also
//! speak, so this isn't locked to one vendor. Blocking HTTP (`ureq`), not
//! `reqwest`: this project's `tokio` dependency only enables
//! `sync, rt, macros, time` (no `net`), and a single blocking POST per
//! `embed` call doesn't justify expanding that.

use serde::{Deserialize, Serialize};

use crate::domain::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};

#[derive(Debug, Serialize)]
struct EmbeddingsRequest<'a> {
    input: &'a [&'a str],
    model: &'a str,
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
    base_url: String,
    model: String,
    api_key: String,
    dimensions: usize,
}

impl RemoteEmbeddingProvider {
    pub fn new(base_url: String, model: String, api_key: String, dimensions: usize) -> Self {
        Self {
            base_url,
            model,
            api_key,
            dimensions,
        }
    }

    fn embeddings_url(&self) -> String {
        format!("{}/embeddings", self.base_url.trim_end_matches('/'))
    }
}

impl EmbeddingProvider for RemoteEmbeddingProvider {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
        let body = EmbeddingsRequest {
            input: texts,
            model: &self.model,
        };

        let mut response = ureq::post(self.embeddings_url())
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .send_json(&body)
            .map_err(|e| EmbeddingError::Http(e.to_string()))?;

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

    #[test]
    fn embeddings_url_strips_trailing_slash() {
        let provider = RemoteEmbeddingProvider::new(
            "https://api.example.com/v1/".to_string(),
            "text-embedding-3-small".to_string(),
            "key".to_string(),
            1536,
        );
        assert_eq!(
            provider.embeddings_url(),
            "https://api.example.com/v1/embeddings"
        );
    }

    #[test]
    fn parses_openai_compatible_response_shape() {
        let json = r#"{"data":[{"embedding":[0.1,0.2,0.3]},{"embedding":[0.4,0.5,0.6]}]}"#;
        let parsed: EmbeddingsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.data[0].embedding, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn request_serializes_with_input_and_model() {
        let texts = ["a", "b"];
        let req = EmbeddingsRequest {
            input: &texts,
            model: "text-embedding-3-small",
        };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(
            json,
            r#"{"input":["a","b"],"model":"text-embedding-3-small"}"#
        );
    }
}
