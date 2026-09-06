//! Bundled Model2Vec static embeddings (`potion-multilingual-128M` int8).
//! Weights live on disk next to the crate / the packaged app — no network
//! at runtime, no ONNX.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use model2vec_rs::model::StaticModel;

use crate::domain::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};

pub(crate) const DIMENSIONS: usize = 256;
const MODEL_DIR_NAME: &str = "potion-multilingual-128M-int8";
const LFS_POINTER_PREFIX: &[u8] = b"version https://git-lfs.github.com/spec/v1";

static MODEL: OnceLock<Result<StaticModel, String>> = OnceLock::new();

pub struct LocalEmbeddingProvider;

impl LocalEmbeddingProvider {
    pub fn try_new() -> Result<Self, EmbeddingError> {
        model()?;
        Ok(Self)
    }
}

impl EmbeddingProvider for LocalEmbeddingProvider {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Embedding>, EmbeddingError> {
        let model = model()?;
        let sentences: Vec<String> = texts.iter().map(|text| (*text).to_string()).collect();
        Ok(model
            .encode_with_args(&sentences, Some(512), 512)
            .into_iter()
            .map(Embedding)
            .collect())
    }

    fn dimensions(&self) -> usize {
        DIMENSIONS
    }
}

fn model() -> Result<&'static StaticModel, EmbeddingError> {
    match MODEL.get_or_init(load_model) {
        Ok(model) => Ok(model),
        Err(err) => Err(EmbeddingError::Provider(err.clone())),
    }
}

fn load_model() -> Result<StaticModel, String> {
    let dir = resolve_model_dir()?;
    let tokenizer = read_model_file(&dir.join("tokenizer.json"))?;
    let weights = read_model_file(&dir.join("model.safetensors"))?;
    let config = read_model_file(&dir.join("config.json"))?;
    StaticModel::from_bytes(&tokenizer, &weights, &config, None)
        .map_err(|e| format!("bundled Model2Vec failed to parse: {e}"))
}

fn read_model_file(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    if bytes.starts_with(LFS_POINTER_PREFIX) {
        return Err(format!(
            "{} is a Git LFS pointer — run `git lfs pull` or scripts/build-embedding-model.py",
            path.display()
        ));
    }
    Ok(bytes)
}

fn resolve_model_dir() -> Result<PathBuf, String> {
    for dir in candidate_model_dirs() {
        if dir.join("model.safetensors").is_file() && dir.join("tokenizer.json").is_file() {
            return Ok(dir);
        }
    }
    Err(format!(
        "bundled Model2Vec files not found (looked for {MODEL_DIR_NAME}/model.safetensors)"
    ))
}

fn candidate_model_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("models")
            .join(MODEL_DIR_NAME),
    );
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("models").join(MODEL_DIR_NAME));
            dirs.push(parent.join("resources").join("models").join(MODEL_DIR_NAME));
            dirs.push(
                parent
                    .join("../Resources")
                    .join("models")
                    .join(MODEL_DIR_NAME),
            );
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_matches_python_parity_fixture() {
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("models")
            .join(MODEL_DIR_NAME)
            .join("parity.json");
        let Ok(raw) = fs::read_to_string(&fixture_path) else {
            return;
        };
        let fixture: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let phrases: Vec<String> = fixture["phrases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let expected: Vec<Vec<f32>> = fixture["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                row.as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_f64().unwrap() as f32)
                    .collect()
            })
            .collect();

        let model = match load_model() {
            Ok(model) => model,
            Err(_) => return,
        };
        let got = model.encode_with_args(&phrases, Some(512), 512);
        assert_eq!(got.len(), expected.len());
        for (i, (a, b)) in got.iter().zip(expected.iter()).enumerate() {
            let cos = cosine(a, b);
            assert!(
                cos >= 0.999,
                "phrase {} cosine {cos} below 0.999",
                phrases[i]
            );
        }
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (na * nb).max(1e-12)
    }
}
