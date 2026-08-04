//! Wraps `fastembed::Bgem3Embedding` (BGE-M3, INT8-quantized ONNX, 1024
//! dense dimensions). Chosen over a smaller multilingual model because this
//! product's primary content language is Russian and BGE-M3 is the
//! stronger multilingual performer of the realistic local options — see
//! `AI_HARNESS.md` for the full tradeoff.
//!
//! Only the dense output is used. BGE-M3 also produces sparse and ColBERT
//! vectors in the same forward pass (useful for hybrid retrieval later),
//! but this vector index is dense-only for now — nothing here prevents
//! surfacing those later without changing the `EmbeddingProvider` trait.

use std::path::PathBuf;
use std::sync::Mutex;

use fastembed::{Bgem3Embedding, Bgem3InitOptions, Bgem3Model};

use crate::domain::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};

const DIMENSIONS: usize = 1024;

/// The Hugging Face repo/file `Bgem3Model::BGEM3Q` resolves to (per
/// `fastembed`'s own `models_list()`) — `services::embedding_model` needs
/// these to check download status via `hf_hub::Cache` without triggering a
/// download itself (`Bgem3Embedding::try_new` would download on a cache
/// miss, which a mere status check must not do).
pub const MODEL_REPO: &str = "gpahal/bge-m3-onnx-int8";
pub const MODEL_FILE: &str = "model_quantized.onnx";

/// `fastembed`'s own default cache dir is a *relative* path
/// (`.fastembed_cache`, resolved against the process's current directory)
/// — fragile for a desktop app where CWD isn't guaranteed stable. Both this
/// provider and `services::embedding_model`'s status check use this
/// absolute path instead, under the same `~/.atlas` directory every other
/// persistent app file lives in.
pub fn model_cache_dir() -> Result<PathBuf, EmbeddingError> {
    let home = dirs::home_dir()
        .ok_or_else(|| EmbeddingError::Message("could not resolve home directory".into()))?;
    Ok(home.join(".atlas").join("models"))
}

/// `Bgem3Embedding::embed` takes `&mut self` — the ONNX Runtime session is
/// mutably borrowed per call — so this wraps it in a `Mutex` to satisfy
/// `EmbeddingProvider: Send + Sync`'s `&self` signature. Inference is
/// already CPU-bound and effectively serialized inside ONNX Runtime, so
/// this isn't giving up meaningful parallelism.
pub struct LocalEmbeddingProvider {
    model: Mutex<Bgem3Embedding>,
}

impl LocalEmbeddingProvider {
    /// Loads the model from `fastembed`'s cache directory. Fails if the
    /// model hasn't been downloaded yet — callers should check
    /// `services::embedding_model::model_status` first.
    pub fn try_new() -> Result<Self, EmbeddingError> {
        let options =
            Bgem3InitOptions::new(Bgem3Model::BGEM3Q).with_cache_dir(model_cache_dir()?);
        // `anyhow::Error::to_string()` only prints the outermost context
        // message, silently dropping the real cause chained underneath it
        // (network/TLS/timeout error) — `{:#}` prints the full chain.
        let model = Bgem3Embedding::try_new(options)
            .map_err(|e| EmbeddingError::Provider(format!("{e:#}")))?;
        Ok(Self {
            model: Mutex::new(model),
        })
    }
}

/// Bounds how many chunks go into one `Bgem3Embedding::embed` call.
/// `fastembed`'s own `batch_size` argument only bounds the ONNX
/// `session.run()` tensor size for its *internal* batching loop — the
/// dense/sparse/ColBERT outputs from every internal batch still accumulate
/// across the **entire** input slice before `embed()` returns. ColBERT in
/// particular is a per-token multi-vector (up to ~511 × 1024 floats *per
/// chunk*, not one vector like dense), and this provider discards it
/// immediately — but calling `embed()` once with an entire sync's pending
/// chunks (thousands, for a real repo) buffers tens of GB of it first, just
/// to throw it away. Looping in small groups here bounds that peak to one
/// group's worth instead of the whole pending set.
const EMBED_BATCH_SIZE: usize = 32;

impl EmbeddingProvider for LocalEmbeddingProvider {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
        let mut model = self
            .model
            .lock()
            .map_err(|_| EmbeddingError::Provider("model lock poisoned".to_string()))?;

        let mut results = Vec::with_capacity(texts.len());
        for group in texts.chunks(EMBED_BATCH_SIZE) {
            let sentences: Vec<&str> = group.to_vec();
            let output = model
                .embed(sentences, Some(EMBED_BATCH_SIZE))
                .map_err(|e| EmbeddingError::Provider(format!("{e:#}")))?;
            // `output`'s sparse/ColBERT vectors drop here, per group, rather
            // than accumulating for the whole `texts` slice.
            results.extend(output.dense.into_iter().map(Embedding));
        }
        Ok(results)
    }

    fn dimensions(&self) -> usize {
        DIMENSIONS
    }
}
