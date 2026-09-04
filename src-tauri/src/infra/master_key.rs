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
use std::sync::Mutex;
use std::time::{Duration, Instant};

use aes_gcm::aead::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
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
/// Records *where* the key went, so a later run can tell "no key yet" from
/// "the key is somewhere I cannot read right now". Holds no key material.
const KEY_STORE_MARKER: &str = "master_key_store.json";

/// Where the master key is kept. Written to `KEY_STORE_MARKER` after every
/// successful resolution, so the record follows reality rather than intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum KeyStore {
    Keychain,
    File,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyStoreMarker {
    store: KeyStore,
}

fn marker_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(KEY_STORE_MARKER))
}

fn recorded_store() -> Option<KeyStore> {
    let path = marker_path().ok()?;
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str::<KeyStoreMarker>(&contents)
        .ok()
        .map(|m| m.store)
}

/// Best-effort: failing to record where the key went must not stop the app
/// from using a key it already holds. The cost of a missing marker is that
/// `get_or_create` falls back to `sealed_blobs_present` to decide whether a
/// key might exist — which is the same answer in every case that matters.
fn record_store(store: KeyStore) {
    if recorded_store() == Some(store) {
        return;
    }
    let Ok(path) = marker_path() else { return };
    let Ok(contents) = serde_json::to_string_pretty(&KeyStoreMarker { store }) else {
        return;
    };
    if let Err(e) = crate::infra::secret_store::write_atomic_private(&path, contents.as_bytes()) {
        eprintln!("[alfa-atlas] could not record the master key location: {e}");
    }
}

pub const KEY_LEN: usize = 32;

/// The master key, wiped from memory when the last holder drops it.
pub(crate) type MasterKey = Zeroizing<[u8; KEY_LEN]>;

fn legacy_key_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(LEGACY_KEY_FILE))
}

/// Whether this build, on this machine, has a keychain that actually
/// persists across `Entry` instances.
///
/// A `true` is cached for the process — a working keychain does not stop
/// working. A `false` deliberately is **not**: unavailability is often
/// temporary (a locked keychain, a Secret Service that starts after the
/// app, an access prompt the user dismissed), and caching it would keep the
/// app on the file fallback for the rest of the session after one bad
/// moment at startup. Re-probing costs three keychain calls, on a path that
/// runs once per credential read rather than per request.
#[cfg(not(test))]
fn keychain_is_usable() -> bool {
    static USABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if USABLE.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    let usable = probe_keychain();
    if usable {
        USABLE.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    usable
}

/// Uncached under test. `$HOME` is stable in a running app but not in the
/// test suite, and on macOS the login keychain is resolved through it — a
/// cached `false` from the first test that ran under `with_temp_home`
/// would otherwise decide the answer for every later test in the process.
#[cfg(test)]
fn keychain_is_usable() -> bool {
    if force_unreachable() {
        return false;
    }
    probe_keychain()
}

/// Test-only switch, consulted by every keychain operation so the simulated
/// condition is faithful: an unreachable keychain fails reads and writes
/// alike, not just the probe.
#[cfg(test)]
fn force_unreachable() -> bool {
    FORCE_UNUSABLE.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(not(test))]
fn force_unreachable() -> bool {
    false
}

#[cfg(test)]
static FORCE_UNUSABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Runs `f` as though this machine had no reachable keychain — a locked
/// one, a dismissed prompt, a Secret Service that has not started. The real
/// condition cannot be produced from a test (locking the developer's login
/// keychain would prompt them for a password), and the branch it guards is
/// the one that decides whether to destroy existing credentials.
///
/// Callers must already hold `with_temp_home`'s lock, which serializes
/// every test that touches this flag.
#[cfg(test)]
pub(crate) fn with_unreachable_keychain<T>(f: impl FnOnce() -> T) -> T {
    use std::sync::atomic::Ordering;
    FORCE_UNUSABLE.store(true, Ordering::Relaxed);
    forget_resolution_for_tests();
    let result = f();
    FORCE_UNUSABLE.store(false, Ordering::Relaxed);
    // The induced failure is recorded in `RESOLUTION`'s cooldown; leaving
    // it set would answer the *next* test from that cooldown instead of
    // resolving.
    forget_resolution_for_tests();
    result
}

/// Drops the process-wide cache so a test starts from a real resolution.
/// `with_temp_home` gives each test its own `~/.atlas`, which a key cached
/// under a previous test's home would silently paper over.
#[cfg(test)]
pub(crate) fn forget_resolution_for_tests() {
    let mut state = RESOLUTION.lock().unwrap_or_else(|e| e.into_inner());
    state.key = None;
    state.failed_at = None;
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
    if force_unreachable() {
        return None;
    }
    // A marker saying the key went into the keychain is itself proof that a
    // real backend stored it, so the probe adds nothing but two more
    // keychain operations — and on a locked keychain, two more password
    // dialogs before the one that matters.
    if recorded_store() != Some(KeyStore::Keychain) && !keychain_is_usable() {
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
    if force_unreachable() || !keychain_is_usable() {
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

/// What a caller sees when a key exists but this run cannot reach it.
///
/// Deliberately an error rather than a new key: `read_secret_file` turns it
/// into "no credential configured", which is recoverable the moment the
/// keychain is reachable again, whereas generating would be permanent.
pub const KEY_UNREACHABLE: &str =
    "the master key is in the OS keychain, which is not reachable right now \
     (locked, access denied, or no Secret Service running) — stored \
     credentials stay encrypted until it is available again";

/// Whether any sealed blob is sitting in `~/.atlas`.
///
/// Evidence that a master key exists even when nothing recorded one — the
/// case for anyone who migrated before `record_store` was introduced.
fn sealed_blobs_present() -> bool {
    let Ok(dir) = settings_store::settings_dir() else {
        return false;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("enc"))
    })
}

/// How long a failed resolution suppresses further keychain access.
///
/// The point is the *absence* of a keychain call while it lasts: on macOS a
/// locked keychain answers with a password dialog, so retrying eagerly
/// turns one dismissed dialog into the next one.
const RETRY_COOLDOWN: Duration = Duration::from_secs(60);

struct Resolution {
    key: Option<MasterKey>,
    failed_at: Option<Instant>,
}

/// Resolved once per process, behind a lock.
///
/// Both properties matter for how often the user sees a password dialog.
/// **Cached**, because `get_or_create` runs on every credential read and
/// `llm_has_api_key` is called once per configured provider each time the
/// LLM settings refresh — without a cache that is one keychain access per
/// provider per refresh. **Locked across the resolution**, because those
/// calls arrive concurrently (the frontend issues them through
/// `Promise.all`), so N callers would otherwise each open their own dialog
/// instead of queueing behind one.
///
/// The trade is that the master key stays resident for the life of the
/// process rather than being re-read and wiped per access. That is the
/// usual "unlock once per session" bargain, and the alternative — a dialog
/// per credential read on a locked keychain — is not one anybody would
/// keep enabled. It buys nothing against reading a live process's memory
/// either way: the key has to be there whenever a request is signed.
static RESOLUTION: Mutex<Resolution> = Mutex::new(Resolution {
    key: None,
    failed_at: None,
});

/// Returns the master key, creating it on first run.
///
/// Resolution order: keychain, then the legacy file (migrating it into the
/// keychain and shredding it when that succeeds), then a freshly generated
/// key. The key bytes are preserved verbatim across migration — every blob
/// sealed by an older build stays decryptable.
///
/// Resolved at most once per process; see `RESOLUTION`. A failure is
/// remembered for `RETRY_COOLDOWN` and answered without touching the
/// keychain at all, so a dismissed password dialog is not immediately
/// followed by another.
///
/// ## Why the last step is guarded
///
/// Once the key has moved into the keychain there is no file left to fall
/// back to, so "keychain said nothing" and "there is no key yet" become
/// indistinguishable by resolution order alone — and they call for opposite
/// actions. A keychain can be unreachable temporarily: locked by a
/// non-default lock timeout, an access prompt the user dismissed, or a
/// Linux session where the Secret Service has not come up yet by the time
/// the app starts. Generating a fresh key in that moment would orphan every
/// sealed blob and trigger an SSH key regeneration, permanently, for a
/// condition that clears on its own.
///
/// So a new key is only minted when nothing indicates one already exists:
/// no store on record, no sealed blob on disk, or a keychain that is
/// working and genuinely empty (the entry was deleted from Keychain Access
/// — unrecoverable, so there is nothing left to protect).
pub(crate) fn get_or_create() -> Result<MasterKey, String> {
    let mut state = RESOLUTION.lock().unwrap_or_else(|e| e.into_inner());

    if let Some(key) = state.key.as_ref() {
        return Ok(key.clone());
    }
    if state
        .failed_at
        .is_some_and(|at| at.elapsed() < RETRY_COOLDOWN)
    {
        return Err(KEY_UNREACHABLE.to_string());
    }

    match resolve() {
        Ok(key) => {
            state.key = Some(key.clone());
            state.failed_at = None;
            Ok(key)
        }
        Err(e) => {
            state.failed_at = Some(Instant::now());
            Err(e)
        }
    }
}

/// Counts actual resolutions, so tests can assert how often the keychain is
/// reached rather than how often it is asked for — the difference between
/// one password dialog and one per credential.
#[cfg(test)]
static RESOLVE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn resolve_count() -> usize {
    RESOLVE_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}

/// The uncached resolution — see `get_or_create`, which is the only caller.
fn resolve() -> Result<MasterKey, String> {
    #[cfg(test)]
    RESOLVE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if let Some(key) = keychain_get() {
        // Authoritative store answered; retire any file left by an older
        // build (or by a run where the keychain was temporarily missing).
        shred_legacy_file();
        record_store(KeyStore::Keychain);
        return Ok(key);
    }

    if let Some(key) = legacy_file_read() {
        if keychain_put(&key) {
            shred_legacy_file();
            record_store(KeyStore::Keychain);
            eprintln!("[alfa-atlas] master key migrated from the file fallback to the OS keychain");
        } else {
            record_store(KeyStore::File);
        }
        return Ok(key);
    }

    if !keychain_is_usable()
        && (recorded_store() == Some(KeyStore::Keychain) || sealed_blobs_present())
    {
        eprintln!("[alfa-atlas] refusing to mint a new master key: {KEY_UNREACHABLE}");
        return Err(KEY_UNREACHABLE.to_string());
    }

    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    OsRng.fill_bytes(key.as_mut_slice());

    if keychain_put(&key) {
        record_store(KeyStore::Keychain);
        eprintln!("[alfa-atlas] created a new master key in the OS keychain");
    } else {
        legacy_file_write(&key)?;
        record_store(KeyStore::File);
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
    forget_resolution_for_tests();
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

    /// The bug this guards: after migration there is no file left, so an
    /// unreachable keychain used to look exactly like a fresh install — and
    /// minting a key then would orphan every sealed blob permanently, for a
    /// condition that clears by itself.
    #[test]
    fn refuses_to_mint_a_key_when_the_keychain_is_only_unreachable() {
        settings_store::test_support::with_temp_home(|| {
            forget_for_tests();
            record_store(KeyStore::Keychain);
            assert!(!legacy_key_path().unwrap().exists());

            let outcome = with_unreachable_keychain(get_or_create);

            assert_eq!(outcome.err().as_deref(), Some(KEY_UNREACHABLE));
            assert!(
                !legacy_key_path().unwrap().exists(),
                "must not have written a fallback key either"
            );
        });
    }

    /// Anyone who migrated before the marker existed has no record — the
    /// sealed blobs themselves are the evidence that a key exists.
    #[test]
    fn refuses_when_sealed_blobs_exist_even_without_a_marker() {
        settings_store::test_support::with_temp_home(|| {
            forget_for_tests();
            let dir = settings_store::ensure_settings_dir().unwrap();
            fs::write(dir.join("jira_credentials.enc"), b"sealed").unwrap();
            assert_eq!(recorded_store(), None);

            let outcome = with_unreachable_keychain(get_or_create);

            assert_eq!(outcome.err().as_deref(), Some(KEY_UNREACHABLE));
        });
    }

    /// The other side of that guard: a machine with no keychain and nothing
    /// to lose must still be able to start.
    #[test]
    fn a_machine_with_no_keychain_and_no_prior_key_still_starts() {
        settings_store::test_support::with_temp_home(|| {
            forget_for_tests();
            settings_store::ensure_settings_dir().unwrap();

            let key = with_unreachable_keychain(get_or_create).expect("should mint a key");
            assert_ne!(*key, [0u8; KEY_LEN]);

            assert!(legacy_key_path().unwrap().exists());
            assert_eq!(recorded_store(), Some(KeyStore::File));
            // And it is stable across restarts on that machine.
            assert_eq!(*with_unreachable_keychain(get_or_create).unwrap(), *key);
        });
    }

    #[test]
    fn the_marker_records_where_the_key_actually_went() {
        settings_store::test_support::with_temp_home(|| {
            forget_for_tests();
            assert_eq!(recorded_store(), None);

            get_or_create().unwrap();
            let expected = if keychain_is_usable() {
                KeyStore::Keychain
            } else {
                KeyStore::File
            };
            assert_eq!(recorded_store(), Some(expected));

            forget_for_tests();
        });
    }

    /// `llm_has_api_key` runs once per configured provider on every LLM
    /// settings refresh, and each call reads a sealed blob. If every one of
    /// those reached the keychain, a locked keychain would answer with one
    /// password dialog per provider per refresh.
    #[test]
    fn many_credential_reads_resolve_the_key_once() {
        settings_store::test_support::with_temp_home(|| {
            forget_for_tests();

            let before = resolve_count();
            let first = *get_or_create().unwrap();
            for _ in 0..20 {
                assert_eq!(*get_or_create().unwrap(), first);
            }
            assert_eq!(resolve_count() - before, 1);

            forget_for_tests();
        });
    }

    /// The frontend issues those per-provider checks through `Promise.all`,
    /// so they arrive at once. Without the lock each would resolve — and
    /// prompt — separately.
    #[test]
    fn concurrent_readers_resolve_the_key_once() {
        settings_store::test_support::with_temp_home(|| {
            forget_for_tests();

            let before = resolve_count();
            let keys: Vec<[u8; KEY_LEN]> = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..8)
                    .map(|_| scope.spawn(|| *get_or_create().unwrap()))
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });

            assert_eq!(resolve_count() - before, 1);
            assert!(keys.windows(2).all(|w| w[0] == w[1]), "all got the same key");

            forget_for_tests();
        });
    }

    /// A dismissed password dialog must not immediately summon the next
    /// one: within the cooldown the answer comes from memory, with no
    /// keychain access at all.
    #[test]
    fn a_failed_resolution_is_not_retried_on_every_call() {
        settings_store::test_support::with_temp_home(|| {
            forget_for_tests();
            record_store(KeyStore::Keychain);

            with_unreachable_keychain(|| {
                let before = resolve_count();
                for _ in 0..10 {
                    assert_eq!(get_or_create().err().as_deref(), Some(KEY_UNREACHABLE));
                }
                assert_eq!(resolve_count() - before, 1);
            });
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
