use crate::domain::metrics::{MetricEvent, MetricsStatus};
use crate::infra::metrics_store;
use crate::services::metrics;

#[tauri::command]
pub fn metrics_status() -> Result<MetricsStatus, String> {
    metrics_store::load()
        .map(MetricsStatus::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn metrics_set_enabled(enabled: bool) -> Result<MetricsStatus, String> {
    metrics::set_enabled(enabled)
        .map(MetricsStatus::from)
        .map_err(|e| e.to_string())
}

/// Sends one event. `ureq` is blocking and `tokio` is built here without
/// the `net` feature, so the send runs on the blocking pool — the same
/// pattern the LLM and embedding providers use.
#[tauri::command]
pub async fn metrics_track(event: MetricEvent) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || metrics::track(event))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
