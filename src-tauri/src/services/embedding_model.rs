//! Local BGE-M3 model download/readiness, decoupled from actually running
//! inference (`infra::embedding_providers::local::LocalEmbeddingProvider`
//! owns that). Two separate concerns on purpose: checking status must never
//! trigger a download, and downloading must not require constructing a
//! provider a caller intends to keep using afterward.

use std::sync::atomic::{AtomicU64, Ordering};

use fastembed::{Bgem3Embedding, Bgem3InitOptions, Bgem3Model};
use hf_hub::{Cache, Repo, RepoType};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::domain::embeddings::{EmbeddingError, ModelStatus};
use crate::infra::embedding_providers::local::{model_cache_dir, MODEL_FILE, MODEL_REPO};

pub const MODEL_DOWNLOAD_PROGRESS_EVENT: &str = "embedding:model-download-progress";

#[derive(Debug, Clone, Serialize)]
struct ModelDownloadProgressPayload {
    progress: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cancelled: Option<bool>,
}

/// `fastembed`'s blocking download (via `hf_hub`) has no interrupt hook, so
/// a "cancel" button can't actually stop in-flight network I/O — the
/// underlying blocking call keeps running on its worker thread regardless.
/// What this tracks instead: a generation counter per download attempt, so
/// once the user cancels (and the UI has already moved on, possibly
/// starting a fresh attempt), a stale attempt's eventual result — success
/// or failure — is reported as cancelled instead of clobbering whatever the
/// UI is showing for the *current* attempt.
#[derive(Default)]
pub struct DownloadState {
    generation: AtomicU64,
    cancelled_generation: AtomicU64,
}

impl DownloadState {
    fn begin(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Marks the most recently started attempt as cancelled. A no-op if
    /// nothing is in flight — the next `begin()` simply won't match it.
    pub fn cancel_current(&self) {
        let current = self.generation.load(Ordering::SeqCst);
        self.cancelled_generation.store(current, Ordering::SeqCst);
    }

    fn is_cancelled(&self, generation: u64) -> bool {
        self.cancelled_generation.load(Ordering::SeqCst) == generation
    }
}

/// Checks whether the BGE-M3 model file is already present in the local
/// cache, **without** triggering a download —
/// `hf_hub::CacheRepo::get` only looks at what's already on disk;
/// `Bgem3Embedding::try_new` would download on a cache miss, which a mere
/// status check must never do as a side effect.
pub fn model_status() -> ModelStatus {
    let cache_dir = match model_cache_dir() {
        Ok(dir) => dir,
        Err(e) => {
            return ModelStatus::Error {
                message: e.to_string(),
            }
        }
    };
    let cache = Cache::new(cache_dir);
    let repo = cache.repo(Repo::new(MODEL_REPO.to_string(), RepoType::Model));
    match repo.get(MODEL_FILE) {
        Some(_) => ModelStatus::Ready,
        None => ModelStatus::NotDownloaded,
    }
}

/// Requests cancellation of whatever download attempt is currently in
/// flight. See `DownloadState` for why this can't be a true abort.
pub fn cancel_download(state: &DownloadState) {
    state.cancel_current();
}

/// Triggers the model download — via `fastembed`'s own
/// `Bgem3Embedding::try_new`, which downloads on a cache miss — and emits
/// [`MODEL_DOWNLOAD_PROGRESS_EVENT`] before/after.
///
/// **Known limitation, not a bug to fix quietly later**: `fastembed`'s
/// `InitOptions::show_download_progress` only toggles an `indicatif`
/// console progress bar — there is no programmatic callback through
/// fastembed's own API for a real byte-level percentage. `hf_hub` itself
/// *does* expose one (`CacheRepo::download_with_progress`), but using it
/// would mean bypassing fastembed's automatic download-on-init and
/// pre-populating its cache directory ourselves to fastembed's exact
/// expected layout — a real, more invasive integration, deliberately
/// deferred rather than guessed at here. For now this emits a coarse
/// two-step progress: `0.0` right before the (blocking, potentially
/// multi-minute) download+load call, `1.0` once it returns successfully.
pub fn download_model(app_handle: &AppHandle, state: &DownloadState) -> Result<(), EmbeddingError> {
    let generation = state.begin();
    emit_progress(app_handle, 0.0, None, None);

    let options = Bgem3InitOptions::new(Bgem3Model::BGEM3Q)
        .with_cache_dir(model_cache_dir()?)
        .with_show_download_progress(false);

    let result = Bgem3Embedding::try_new(options);

    if state.is_cancelled(generation) {
        emit_progress(app_handle, 0.0, None, Some(true));
        return Err(EmbeddingError::Message("download cancelled".to_string()));
    }

    match result {
        Ok(_) => {
            emit_progress(app_handle, 1.0, None, None);
            Ok(())
        }
        Err(e) => {
            // `anyhow::Error::to_string()` only prints the outermost
            // context ("Failed to retrieve model_quantized.onnx"),
            // silently dropping the actual cause (network/TLS/timeout
            // error) chained underneath it. `{:#}` prints the full chain.
            let message = format!("{e:#}");
            emit_progress(app_handle, 0.0, Some(message.clone()), None);
            Err(EmbeddingError::Provider(message))
        }
    }
}

fn emit_progress(app_handle: &AppHandle, progress: f32, error: Option<String>, cancelled: Option<bool>) {
    let _ = app_handle.emit(
        MODEL_DOWNLOAD_PROGRESS_EVENT,
        &ModelDownloadProgressPayload {
            progress,
            error,
            cancelled,
        },
    );
}
