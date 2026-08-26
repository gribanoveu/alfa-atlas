//! Local BGE-M3 model download/readiness, decoupled from actually running
//! inference (`infra::embedding_providers::local::LocalEmbeddingProvider`
//! owns that). Two separate concerns on purpose: checking status must never
//! trigger a download, and downloading must not require constructing a
//! provider a caller intends to keep using afterward.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use fastembed::{Bgem3Embedding, Bgem3InitOptions, Bgem3Model};
use hf_hub::api::sync::ApiBuilder;
use hf_hub::api::Progress;
use hf_hub::{Cache, Repo, RepoType};

use crate::domain::embeddings::{
    EmbeddingError, ModelDownloadProgress, ModelDownloadSink, ModelStatus,
};
use crate::infra::embedding_providers::local::{model_cache_dir, MODEL_FILE, MODEL_REPO};

/// Mirrors `HF_ENDPOINT`'s hardcoded default inside `hf_hub`/`fastembed`
/// itself — not part of either crate's public API, so this needs its own
/// copy to reproduce the same `Api`/`ApiRepo` construction
/// `fastembed::common::pull_from_hf` does internally (see
/// `download_weights_with_progress`'s doc comment for why that reproduction
/// has to match exactly, not just approximately).
const HF_ENDPOINT_DEFAULT: &str = "https://huggingface.co";

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

/// Triggers the model download and emits [`MODEL_DOWNLOAD_PROGRESS_EVENT`]
/// with real byte-level progress while the ~570MB weights file downloads,
/// then hands off to `fastembed`'s own `Bgem3Embedding::try_new` to load it
/// (plus fetch the handful of small tokenizer files BGE-M3 also needs).
///
/// `fastembed`'s `InitOptions::show_download_progress` only toggles an
/// `indicatif` *console* progress bar — there's no programmatic callback
/// through fastembed's own API for a real percentage. `hf_hub` (the crate
/// fastembed itself downloads through) does expose one
/// (`ApiRepo::download_with_progress`), so `download_weights_with_progress`
/// below calls that directly for [`MODEL_FILE`] first — writing into the
/// exact same on-disk cache layout (`Api`'s blob/pointer/ref files)
/// `Bgem3Embedding::try_new`'s own internal `ApiRepo::get` will look for —
/// so that subsequent call finds a cache hit and does no redundant network
/// work, just a fast local lookup. The small tokenizer files
/// `try_new` fetches after that stay coarse (no dedicated progress) since
/// they're negligible next to the weights file's size — not worth a second
/// progress-reporting path for a few hundred KB.
pub fn download_model(progress: &ModelDownloadSink, state: &DownloadState) -> Result<(), EmbeddingError> {
    let generation = state.begin();
    emit_progress(progress, 0.0, None, None);

    let cache_dir = model_cache_dir()?;
    let result = download_weights_with_progress(progress, cache_dir.clone()).and_then(|_| {
        let options = Bgem3InitOptions::new(Bgem3Model::BGEM3Q)
            .with_cache_dir(cache_dir)
            .with_show_download_progress(false);
        Bgem3Embedding::try_new(options)
            // `anyhow::Error::to_string()` only prints the outermost
            // context ("Failed to retrieve model_quantized.onnx"),
            // silently dropping the actual cause (network/TLS/timeout
            // error) chained underneath it. `{:#}` prints the full chain.
            .map_err(|e| EmbeddingError::Provider(format!("{e:#}")))
    });

    if state.is_cancelled(generation) {
        emit_progress(progress, 0.0, None, Some(true));
        return Err(EmbeddingError::Message("download cancelled".to_string()));
    }

    match result {
        Ok(_) => {
            emit_progress(progress, 1.0, None, None);
            Ok(())
        }
        Err(e) => {
            let message = e.to_string();
            emit_progress(progress, 0.0, Some(message.clone()), None);
            Err(e)
        }
    }
}

/// Pre-populates [`MODEL_FILE`]'s cache entry via `hf_hub`'s
/// `ApiRepo::download_with_progress`, emitting real byte progress as it
/// goes. Constructs the `Api`/`ApiRepo` exactly the way `fastembed`'s own
/// (private) `common::pull_from_hf` does — same `HF_HOME`/`HF_ENDPOINT`
/// env var precedence, same `ApiBuilder` calls — because `Api`/`Cache`
/// resolve a fixed on-disk layout from these inputs; anything less than an
/// exact match would make this populate a *different* cache entry than the
/// one `Bgem3Embedding::try_new` looks up afterward, turning this into
/// wasted bandwidth instead of a shared cache hit. A no-op (near-instant)
/// if the file's already cached — `download_with_progress` checks the
/// blob's etag-keyed path itself, same as a plain cache lookup would.
fn download_weights_with_progress(
    progress: &ModelDownloadSink,
    default_cache_dir: PathBuf,
) -> Result<(), EmbeddingError> {
    let cache_dir = std::env::var("HF_HOME")
        .map(PathBuf::from)
        .unwrap_or(default_cache_dir);
    let endpoint = std::env::var("HF_ENDPOINT").unwrap_or_else(|_| HF_ENDPOINT_DEFAULT.to_string());

    let api = ApiBuilder::new()
        .with_cache_dir(cache_dir)
        .with_endpoint(endpoint)
        .with_progress(false)
        .build()
        .map_err(|e| EmbeddingError::Provider(e.to_string()))?;
    let repo = api.model(MODEL_REPO.to_string());

    repo.download_with_progress(MODEL_FILE, ProgressReporter::new(progress))
        .map_err(|e| EmbeddingError::Provider(e.to_string()))?;
    Ok(())
}

/// Reports [`MODEL_FILE`]'s download progress via
/// [`MODEL_DOWNLOAD_PROGRESS_EVENT`], throttled to roughly 1% steps —
/// `hf_hub::api::Progress::update` fires once per raw read chunk (a few
/// tens of KB at a time for a ~570MB file), so emitting a Tauri IPC event
/// on every call would flood the frontend with tens of thousands of events
/// for one download.
struct ProgressReporter<'a> {
    progress: &'a ModelDownloadSink,
    total: usize,
    downloaded: usize,
    last_emitted_fraction: f32,
}

impl<'a> ProgressReporter<'a> {
    fn new(progress: &'a ModelDownloadSink) -> Self {
        Self {
            progress,
            total: 0,
            downloaded: 0,
            last_emitted_fraction: 0.0,
        }
    }
}

impl Progress for ProgressReporter<'_> {
    fn init(&mut self, size: usize, _filename: &str) {
        self.total = size;
        self.downloaded = 0;
        self.last_emitted_fraction = 0.0;
    }

    fn update(&mut self, size: usize) {
        self.downloaded += size;
        if self.total == 0 {
            return;
        }
        let fraction = (self.downloaded as f32 / self.total as f32).min(1.0);
        if should_emit_progress(self.last_emitted_fraction, fraction) {
            self.last_emitted_fraction = fraction;
            emit_progress(self.progress, fraction, None, None);
        }
    }

    fn finish(&mut self) {}
}

/// The throttling decision itself, pulled out of `ProgressReporter::update`
/// as a pure function so it's testable without a live download (the sink
/// itself is trivial to fake, but the surrounding `hf_hub` machinery is not,
/// e.g. `services::embedding_sync::sync_backlog_batch`'s tests call it directly
/// so the throttling decision is tested here rather than through the
/// download wrappers above it).
fn should_emit_progress(last_emitted_fraction: f32, fraction: f32) -> bool {
    fraction - last_emitted_fraction >= 0.01 || fraction >= 1.0
}

fn emit_progress(
    sink: &ModelDownloadSink,
    progress: f32,
    error: Option<String>,
    cancelled: Option<bool>,
) {
    sink(ModelDownloadProgress { progress, error, cancelled });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_emit_progress_fires_on_a_one_percent_step() {
        assert!(should_emit_progress(0.0, 0.01));
        assert!(!should_emit_progress(0.0, 0.005));
    }

    #[test]
    fn should_emit_progress_always_fires_on_completion_even_for_a_tiny_step() {
        // A file whose last chunk pushes `fraction` from e.g. 0.999 to 1.0
        // must still emit — the UI needs a definitive "done" tick, not just
        // "close enough to 100%".
        assert!(should_emit_progress(0.999, 1.0));
    }

    #[test]
    fn should_emit_progress_does_not_regress_on_a_resumed_download() {
        // `hf_hub`'s `download_from` calls `progress.update(current)` once
        // up front to account for bytes a resumed download already has on
        // disk — `last_emitted_fraction` starts at `0.0` (reset in `init`)
        // regardless, so the first post-resume update should still emit if
        // it clears the 1% threshold from zero.
        assert!(should_emit_progress(0.0, 0.42));
    }
}
