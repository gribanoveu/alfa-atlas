use std::fs;
use std::path::PathBuf;

use crate::domain::settings::{AppSettings, SettingsError};

const SETTINGS_DIR_NAME: &str = ".atlas";
const SETTINGS_FILE_NAME: &str = "settings.json";

pub fn settings_dir() -> Result<PathBuf, SettingsError> {
    let home = dirs::home_dir().ok_or(SettingsError::HomeDirUnavailable)?;
    Ok(home.join(SETTINGS_DIR_NAME))
}

pub fn settings_path() -> Result<PathBuf, SettingsError> {
    Ok(settings_dir()?.join(SETTINGS_FILE_NAME))
}

/// Loads settings from `~/.atlas/settings.json`.
/// Missing file yields `AppSettings::default()`.
pub fn load() -> Result<AppSettings, SettingsError> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }

    let contents = fs::read_to_string(&path).map_err(SettingsError::Read)?;
    let settings = serde_json::from_str(&contents).map_err(SettingsError::Parse)?;
    Ok(settings)
}

pub fn save(settings: &AppSettings) -> Result<(), SettingsError> {
    let dir = settings_dir()?;
    fs::create_dir_all(&dir).map_err(SettingsError::CreateDir)?;

    let path = dir.join(SETTINGS_FILE_NAME);
    let contents = serde_json::to_string_pretty(settings).map_err(SettingsError::Serialize)?;
    fs::write(&path, contents).map_err(SettingsError::Write)?;
    Ok(())
}

/// Shared test seam for anything that resolves through `settings_dir()`
/// (`chat_store`, `services::embedding_state::resolve_index_paths`, …).
/// `$HOME` is process-global and `cargo test` runs on multiple threads by
/// default, so every test module that redirects it must serialize against
/// every *other* one too, not just against tests in its own module —
/// hence one shared lock/helper here rather than a private copy per
/// module (a private copy only protects a module against itself).
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());
    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Redirects `settings_dir()`'s effect for the duration of `f` by
    /// pointing `$HOME` at a fresh temp dir, holding `HOME_ENV_LOCK` for
    /// the whole swap-run-restore round trip.
    pub(crate) fn with_temp_home<T>(f: impl FnOnce() -> T) -> T {
        let _guard = HOME_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!("alfa-atlas-test-home-{nanos}-{n}"));
        std::fs::create_dir_all(&home).unwrap();
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);
        let result = f();
        match previous {
            Some(p) => std::env::set_var("HOME", p),
            None => std::env::remove_var("HOME"),
        }
        std::fs::remove_dir_all(&home).ok();
        result
    }
}
