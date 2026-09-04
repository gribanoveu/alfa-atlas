//! The app's master key (KEK) — the single AES-256 key every sealed blob
//! under `~/.atlas` is encrypted with (`infra::secret_store`).
//!
//! The authoritative store is the **OS keychain** (macOS Keychain, Windows
//! Credential Manager, Linux Secret Service) via the `keyring` crate. The
//! file at `~/.atlas/.enc_key` is a *fallback only*, kept for platforms
//! where no keychain is reachable — and migrated away from, then shredded,
//! the first time a keychain turns out to be usable.
//!
//! Why this matters: the file fallback stores the key in plaintext next to
//! the ciphertext it unlocks, so anything that can copy `~/.atlas` — a
//! backup, a cloud-synced home directory, an "email me your config" — gets
//! both halves. Moving the key into the keychain removes it from every
//! file-copy vector. It does **not** defend against malware running as the
//! same user: on all three platforms an unlocked keychain is readable by
//! that user's processes. That tier needs a master password, which is a
//! separate change.
//!
//! ## Detecting a store that only pretends to work
//!
//! `keyring` 3.x makes its platform backends opt-in features; with none
//! enabled it silently compiles to an in-memory mock whose writes return
//! `Ok(())` and evaporate at process exit. A missing Secret Service on
//! Linux fails less quietly but just as fatally. So the keychain is never
//! trusted on the strength of a successful write: `probe_keychain` writes a
//! marker through one `Entry` and reads it back through a *fresh* one,
//! which the mock cannot satisfy (it allocates independent state per
//! `Entry`), and only then is the store considered real.

use std::fs;
use std::path::PathBuf;

use aes_gcm::aead::OsRng;
use rand::RngCore;
use zeroize::Zeroizing;

use crate::infra::settings_store;

#[cfg(not(test))]
const KEYRING_SERVICE: &str = "com.eugene.alfa-atlas";
/// Tests get their own service name. `with_temp_home` can redirect the
/// *file* fallback but not the keychain, and a test run that generated a
/// key under the production name would overwrite the user's real one —
/// after which the app would decrypt its own blobs with the wrong key and
/// every stored token would be lost.
#[cfg(test)]
const KEYRING_SERVICE: &str = "com.eugene.alfa-atlas.tests";

const KEYRING_USER: &str = "encryption-key";
/// Account used only by `probe_keychain`, never for real key material.
const KEYRING_PROBE_USER: &str = "backend-probe";
/// Pre-keychain fallback location; also the migration source.
const LEGACY_KEY_FILE: &str = ".enc_key";

pub const KEY_LEN: usize = 32;

/// The master key, wiped from memory when the last holder drops it.
pub(crate) type MasterKey = Zeroizing<[u8; KEY_LEN]>;

fn legacy_key_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(LEGACY_KEY_FILE))
}

/// Whether this build, on this machine, has a keychain that actually
/// persists across `Entry` instances. Probed once per process.
#[cfg(not(test))]
fn keychain_is_usable() -> bool {
    static USABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *USABLE.get_or_init(probe_keychain)
}

/// Uncached under test. `$HOME` is stable in a running app but not in the
/// test suite, and on macOS the login keychain is resolved through it — a
/// cached `false` from the first test that ran under `with_temp_home`
/// would otherwise decide the answer for every later test in the process.
#[cfg(test)]
fn keychain_is_usable() -> bool {
    probe_keychain()
}

fn probe_keychain() -> bool {
    let marker: [u8; 8] = {
        let mut b = [0u8; 8];
        OsRng.fill_bytes(&mut b);
        b
    };

    let writer = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_PROBE_USER) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[alfa-atlas] keychain unavailable (Entry::new: {e}) — using file fallback");
            return false;
        }
    };
    if let Err(e) = writer.set_secret(&marker) {
        eprintln!("[alfa-atlas] keychain unavailable (write: {e}) — using file fallback");
        return false;
    }

    // Deliberately a *new* Entry: the mock store keeps its value inside the
    // Entry that wrote it, so only a real backend can answer this.
    let usable = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_PROBE_USER) {
        Ok(reader) => matches!(reader.get_secret(), Ok(v) if v == marker),
        Err(_) => false,
    };
    let _ = writer.delete_credential();

    if !usable {
        eprintln!(
            "[alfa-atlas] keychain writes do not persist (mock store or no Secret Service) \
             — using file fallback"
        );
    }
    usable
}

fn keychain_get() -> Option<MasterKey> {
    if !keychain_is_usable() {
        return None;
    }
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?;
    let secret = match entry.get_secret().map(Zeroizing::new) {
        Ok(s) => s,
        Err(keyring::Error::NoEntry) => return None,
        Err(e) => {
            eprintln!("[alfa-atlas] keychain read failed: {e}");
            return None;
        }
    };
    match <[u8; KEY_LEN]>::try_from(secret.as_slice()) {
        Ok(key) => Some(Zeroizing::new(key)),
        Err(_) => {
            eprintln!(
                "[alfa-atlas] keychain holds a {}-byte key, expected {KEY_LEN} — ignoring it",
                secret.len()
            );
            None
        }
    }
}

/// Stores `key` and verifies it reads back, so a caller may only shred the
/// file fallback once the keychain has demonstrably taken over.
fn keychain_put(key: &[u8; KEY_LEN]) -> bool {
    if !keychain_is_usable() {
        return false;
    }
    let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) else {
        return false;
    };
    if let Err(e) = entry.set_secret(key) {
        eprintln!("[alfa-atlas] keychain write failed: {e}");
        return false;
    }
    keychain_get().is_some_and(|stored| stored.as_slice() == key.as_slice())
}

fn legacy_file_read() -> Option<MasterKey> {
    let path = legacy_key_path().ok()?;
    let bytes = Zeroizing::new(fs::read(&path).ok()?);
    match <[u8; KEY_LEN]>::try_from(bytes.as_slice()) {
        Ok(key) => Some(Zeroizing::new(key)),
        Err(_) => {
            eprintln!(
                "[alfa-atlas] {} holds {} bytes, expected {KEY_LEN} — ignoring it",
                path.display(),
                bytes.len()
            );
            None
        }
    }
}

fn legacy_file_write(key: &[u8; KEY_LEN]) -> Result<(), String> {
    let path = legacy_key_path()?;
    settings_store::ensure_settings_dir().map_err(|e| e.to_string())?;
    fs::write(&path, key).map_err(|e| format!("failed to write encryption key file: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&path, perms);
        }
    }

    Ok(())
}

/// Overwrites the fallback file with zeros before unlinking it.
///
/// The overwrite is a courtesy, not a guarantee — on a copy-on-write
/// filesystem (APFS, btrfs) or an SSD with wear levelling the old blocks
/// may survive. It costs nothing and helps on the filesystems where it
/// does work; the unlink is the part that matters.
fn shred_legacy_file() {
    let Ok(path) = legacy_key_path() else { return };
    if !path.exists() {
        return;
    }
    let _ = fs::write(&path, [0u8; KEY_LEN]);
    if let Err(e) = fs::remove_file(&path) {
        eprintln!("[alfa-atlas] failed to remove {}: {e}", path.display());
    }
}

/// Returns the master key, creating it on first run.
///
/// Resolution order: keychain, then the legacy file (migrating it into the
/// keychain and shredding it when that succeeds), then a freshly generated
/// key. The key bytes are preserved verbatim across migration — every blob
/// sealed by an older build stays decryptable.
pub(crate) fn get_or_create() -> Result<MasterKey, String> {
    if let Some(key) = keychain_get() {
        // Authoritative store answered; retire any file left by an older
        // build (or by a run where the keychain was temporarily missing).
        shred_legacy_file();
        return Ok(key);
    }

    if let Some(key) = legacy_file_read() {
        if keychain_put(&key) {
            shred_legacy_file();
            eprintln!("[alfa-atlas] master key migrated from the file fallback to the OS keychain");
        }
        return Ok(key);
    }

    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    OsRng.fill_bytes(key.as_mut_slice());

    if keychain_put(&key) {
        eprintln!("[alfa-atlas] created a new master key in the OS keychain");
    } else {
        legacy_file_write(&key)?;
        eprintln!("[alfa-atlas] created a new master key in the file fallback");
    }

    Ok(key)
}

/// Removes the test-only keychain entry so a `cargo test` run leaves
/// nothing behind in the developer's keychain.
#[cfg(test)]
pub(crate) fn forget_for_tests() {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probe must agree with what the crate was actually built with:
    /// on the three shipping platforms `Cargo.toml` enables a native
    /// backend, so a `false` here means those features were dropped and
    /// every secret would silently fall back to the plaintext key file.
    #[test]
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn native_keychain_backend_is_compiled_in() {
        // Needs the real `$HOME`: see `with_real_home`.
        settings_store::test_support::with_real_home(|| {
            assert!(
                keychain_is_usable(),
                "keyring has no working native backend — check the per-target \
                 `features` on the `keyring` dependency in Cargo.toml"
            );
        });
    }

    #[test]
    fn legacy_file_rejects_a_wrong_length_key() {
        settings_store::test_support::with_temp_home(|| {
            let path = legacy_key_path().unwrap();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"too short").unwrap();
            assert!(legacy_file_read().is_none());
        });
    }

    #[test]
    fn legacy_file_round_trips_and_shreds() {
        settings_store::test_support::with_temp_home(|| {
            let key = [3u8; KEY_LEN];
            legacy_file_write(&key).unwrap();
            assert_eq!(legacy_file_read().as_deref(), Some(&key));

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(legacy_key_path().unwrap())
                    .unwrap()
                    .permissions()
                    .mode();
                assert_eq!(mode & 0o777, 0o600);
            }

            shred_legacy_file();
            assert!(!legacy_key_path().unwrap().exists());
            assert!(legacy_file_read().is_none());
        });
    }

    /// The migration must hand back the *same* key bytes it found in the
    /// file — every `.enc` blob already on disk was sealed with them, so a
    /// freshly generated key here would silently destroy the user's stored
    /// tokens rather than move them.
    #[test]
    fn migrating_a_legacy_file_preserves_the_key_and_retires_the_file() {
        settings_store::test_support::with_temp_home(|| {
            forget_for_tests();

            let existing = [0xABu8; KEY_LEN];
            legacy_file_write(&existing).unwrap();

            assert_eq!(*get_or_create().unwrap(), existing);

            if keychain_is_usable() {
                assert!(
                    !legacy_key_path().unwrap().exists(),
                    "file fallback should be shredded once the keychain holds the key"
                );
                // Still the same key on the next start, now from the keychain.
                assert_eq!(*get_or_create().unwrap(), existing);
            } else {
                assert!(legacy_key_path().unwrap().exists());
            }

            forget_for_tests();
        });
    }

    /// The branch that actually matters, exercised end to end: the key
    /// moves into the keychain, the plaintext file is gone, and the next
    /// start reads the *same* key back out of the keychain.
    ///
    /// `~/.atlas` is isolated in the temp home while the keychain stays
    /// reachable through the symlink `with_temp_home` sets up, and the
    /// test-only `KEYRING_SERVICE` keeps the user's own key untouched.
    #[test]
    #[cfg(target_os = "macos")]
    fn migration_moves_the_key_into_the_keychain_and_shreds_the_file() {
        settings_store::test_support::with_temp_home(|| {
            forget_for_tests();
            assert!(keychain_is_usable(), "keychain should be reachable again");

            let existing = [0xCDu8; KEY_LEN];
            legacy_file_write(&existing).unwrap();

            assert_eq!(*get_or_create().unwrap(), existing, "key bytes must survive");
            assert!(
                !legacy_key_path().unwrap().exists(),
                "plaintext key file must be shredded after migration"
            );
            assert_eq!(keychain_get().as_deref(), Some(&existing), "keychain must hold the key");
            assert_eq!(*get_or_create().unwrap(), existing, "and serve it next start");

            forget_for_tests();
        });
    }

    #[test]
    fn a_fresh_install_gets_a_stable_key() {
        settings_store::test_support::with_temp_home(|| {
            forget_for_tests();
            let first = *get_or_create().unwrap();
            assert_ne!(first, [0u8; KEY_LEN]);
            assert_eq!(*get_or_create().unwrap(), first);
            forget_for_tests();
        });
    }

    #[test]
    fn shredding_a_missing_file_is_not_an_error() {
        settings_store::test_support::with_temp_home(|| {
            shred_legacy_file();
            assert!(!legacy_key_path().unwrap().exists());
        });
    }
}
