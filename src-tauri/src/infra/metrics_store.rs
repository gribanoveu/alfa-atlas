//! Persists metrics state in `~/.atlas/metrics.json`. Same shape as
//! `infra::onboarding_store` — a single small JSON document next to
//! `settings.json`, deliberately its own file so a user who wants to reset
//! their anonymous install id can delete exactly that and nothing else.

use std::fs;

use uuid::Uuid;

use crate::domain::metrics::MetricsState;
use crate::domain::settings::SettingsError;

const METRICS_FILE_NAME: &str = "metrics.json";

fn metrics_path() -> Result<std::path::PathBuf, SettingsError> {
    let dir = crate::infra::settings_store::settings_dir()?;
    Ok(dir.join(METRICS_FILE_NAME))
}

/// Loads `MetricsState` from `~/.atlas/metrics.json`. A missing file
/// yields the default state *without* writing it — the file is only
/// created once there is something worth recording, so merely launching
/// the app with metrics disabled leaves no trace on disk.
pub fn load() -> Result<MetricsState, SettingsError> {
    let path = metrics_path()?;
    if !path.exists() {
        return Ok(MetricsState::default());
    }
    let contents = fs::read_to_string(&path).map_err(SettingsError::Read)?;
    let state = serde_json::from_str(&contents).map_err(SettingsError::Parse)?;
    Ok(state)
}

pub fn save(state: &MetricsState) -> Result<(), SettingsError> {
    let path = metrics_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(SettingsError::CreateDir)?;
    }
    let contents = serde_json::to_string_pretty(state).map_err(SettingsError::Serialize)?;
    fs::write(&path, contents).map_err(SettingsError::Write)?;
    Ok(())
}

/// Returns the persisted install id, generating and saving one on first
/// call. The id is a bare UUID v4 — no hostname, user name or repository
/// is mixed in, so it identifies an installation and nothing else.
pub fn ensure_install_id() -> Result<(MetricsState, String), SettingsError> {
    let mut state = load()?;
    if let Some(id) = state.install_id.clone() {
        return Ok((state, id));
    }
    let id = Uuid::new_v4().to_string();
    state.install_id = Some(id.clone());
    save(&state)?;
    Ok((state, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings_store::test_support::with_temp_home;

    #[test]
    fn load_on_a_fresh_profile_returns_the_default_without_creating_a_file() {
        with_temp_home(|| {
            let state = load().unwrap();
            assert_eq!(state, MetricsState::default());
            assert!(
                !metrics_path().unwrap().exists(),
                "reading state must not create the file"
            );
        });
    }

    #[test]
    fn ensure_install_id_generates_once_and_then_is_stable() {
        with_temp_home(|| {
            let (_, first) = ensure_install_id().unwrap();
            let (_, second) = ensure_install_id().unwrap();
            assert_eq!(first, second);
            assert_eq!(Uuid::parse_str(&first).unwrap().to_string(), first);
            assert_eq!(load().unwrap().install_id.as_deref(), Some(first.as_str()));
        });
    }

    #[test]
    fn save_then_load_round_trips() {
        with_temp_home(|| {
            let state = MetricsState {
                install_id: Some("fixed-id".to_string()),
                install_reported_at: Some(42),
                enabled: false,
            };
            save(&state).unwrap();
            assert_eq!(load().unwrap(), state);
        });
    }
}
