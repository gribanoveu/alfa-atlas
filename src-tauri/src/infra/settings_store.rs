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

/// Creates `~/.atlas` if missing and narrows it to `0o700` on unix.
///
/// The directory holds sealed credential blobs (`*.enc`), the SSH private
/// key and the chat/tool-call databases, so "other" and "group" have no
/// business reading it. `create_dir_all` applies the process umask, which
/// on a default macOS/Linux account yields `0o755` — hence the explicit
/// tightening, applied on every call so an already-existing directory
/// created by an older build gets fixed too.
///
/// Permission errors are swallowed: the directory may legitimately be
/// owned by a different uid in exotic setups, and failing to *narrow*
/// permissions is never a reason to refuse to start.
pub fn ensure_settings_dir() -> Result<PathBuf, SettingsError> {
    let dir = settings_dir()?;
    fs::create_dir_all(&dir).map_err(SettingsError::CreateDir)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&dir) {
            let mut perms = meta.permissions();
            if perms.mode() & 0o077 != 0 {
                perms.set_mode(0o700);
                let _ = fs::set_permissions(&dir, perms);
            }
        }
    }

    Ok(dir)
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
    let dir = ensure_settings_dir()?;

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

    /// Runs `f` holding `HOME_ENV_LOCK` but leaving `$HOME` alone, for
    /// tests that need the *real* home directory.
    ///
    /// On macOS the keychain resolves the login keychain through `$HOME`,
    /// so a `with_temp_home` running concurrently makes any keychain call
    /// fail with "A default keychain could not be found" — taking the same
    /// lock is what keeps those tests from colliding.
    pub(crate) fn with_real_home<T>(f: impl FnOnce() -> T) -> T {
        let _guard = HOME_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        f()
    }

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
        link_keychains_into(&home, previous.as_deref());
        std::env::set_var("HOME", &home);
        // The master key is cached per process; a key resolved under some
        // other test's `~/.atlas` must not answer for this one.
        crate::infra::master_key::forget_resolution_for_tests();
        let result = f();
        match previous {
            Some(p) => std::env::set_var("HOME", p),
            None => std::env::remove_var("HOME"),
        }
        std::fs::remove_dir_all(&home).ok();
        result
    }

    /// Points the temp home's `Library/Keychains` at the real one.
    ///
    /// macOS resolves the login keychain through `$HOME`, and
    /// Security.framework caches the result *per process*: the first
    /// keychain call made under a temp home without this link resolves to
    /// "no default keychain" and every later call in that process keeps
    /// failing, however the home is arranged by then. That made keychain
    /// coverage depend on test order — a test passing alone and failing in
    /// the suite. Linking unconditionally keeps every temp home
    /// keychain-capable, so the answer no longer depends on who ran first.
    ///
    /// Only a symlink is created, and `remove_dir_all` does not traverse
    /// symlinks, so teardown can never reach the real keychains.
    #[cfg(target_os = "macos")]
    fn link_keychains_into(home: &std::path::Path, real_home: Option<&std::ffi::OsStr>) {
        let Some(real_home) = real_home else { return };
        let target = std::path::Path::new(real_home).join("Library/Keychains");
        if !target.is_dir() {
            return;
        }
        if std::fs::create_dir_all(home.join("Library")).is_ok() {
            let _ = std::os::unix::fs::symlink(target, home.join("Library/Keychains"));
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn link_keychains_into(_home: &std::path::Path, _real_home: Option<&std::ffi::OsStr>) {}
}
