//! Encrypted storage for configured LLM providers' API keys. Sealed by
//! `infra::secret_store` under the app master key, like every other secret
//! in `~/.atlas`, and keeps the "write-only from the frontend's
//! perspective" contract — nothing in `commands::llm` ever returns a
//! decrypted key over IPC, only a boolean `has_api_key` status.
//!
//! One structural difference from its embedding sibling: this is keyed by
//! provider id rather than holding a single global key, since more than one
//! LLM provider can be configured at once. Stored as one encrypted
//! `HashMap<provider_id, api_key>` blob, not one file per provider —
//! simpler file management for what's always a handful of entries, at the
//! cost of `has_api_key` needing the whole blob to answer about one
//! provider. That cost is what `configured_ids` caches away.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use secrecy::SecretString;
use zeroize::{Zeroize, Zeroizing};

use crate::infra::secret_store::{self, SecretPurpose};
use crate::infra::settings_store;

const CREDENTIALS_FILE: &str = "llm_credentials.enc";
const PURPOSE: SecretPurpose = SecretPurpose::LlmCredentials;

fn credentials_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(CREDENTIALS_FILE))
}

/// The whole decrypted provider→key map, with every key wiped when the map
/// is dropped.
///
/// `serde_json` has to deserialize into plain `String`s, so the API keys do
/// land in ordinary heap allocations on the way in; what this wrapper
/// guarantees is that they do not simply get freed and left behind. Nothing
/// hands this type out — callers get a single `SecretString` — so the
/// window is one function call wide.
#[derive(Default)]
struct Credentials(HashMap<String, String>);

impl Deref for Credentials {
    type Target = HashMap<String, String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Credentials {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for Credentials {
    fn drop(&mut self) {
        for key in self.0.values_mut() {
            key.zeroize();
        }
    }
}

/// Counts full decrypts, so the cache can be asserted on directly.
#[cfg(test)]
static LOAD_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
fn load_count() -> usize {
    LOAD_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}

/// Missing file / stale key / corrupt data all degrade to an empty map —
/// mirrors `get_api_key`'s `Option`-returning, never-panics contract on the
/// embedding-credentials sibling.
fn load_all() -> Credentials {
    #[cfg(test)]
    LOAD_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let Ok(path) = credentials_path() else {
        return Credentials::default();
    };
    let Some(plain) = secret_store::read_secret_file(&path, PURPOSE) else {
        return Credentials::default();
    };
    Credentials(serde_json::from_slice(&plain).unwrap_or_default())
}

fn save_all(map: &Credentials) -> Result<(), String> {
    let plain = Zeroizing::new(
        serde_json::to_vec(&map.0)
            .map_err(|e| format!("failed to serialize LLM credentials: {e}"))?,
    );
    let result = secret_store::write_secret_file(&credentials_path()?, PURPOSE, &plain);
    *configured_ids().lock().unwrap_or_else(|e| e.into_inner()) = None;
    result
}

/// Which providers have a key, cached against the credential file's size
/// and mtime.
///
/// `has_api_key` runs once per configured provider on every LLM settings
/// refresh, and each call decrypted the whole blob to answer a boolean.
/// Only the *set of ids* is needed for that, and a set of ids is not secret
/// — which is the reason this may be cached at all, where the keys
/// themselves may not.
///
/// Revalidated by `stat` rather than trusted indefinitely, so a file
/// replaced from outside the app is picked up. `save_all` also clears it
/// outright, so our own writes never depend on mtime granularity.
fn configured_ids() -> &'static Mutex<Option<(FileStamp, HashSet<String>)>> {
    static IDS: OnceLock<Mutex<Option<(FileStamp, HashSet<String>)>>> = OnceLock::new();
    IDS.get_or_init(|| Mutex::new(None))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

fn stamp(path: &Path) -> Option<FileStamp> {
    let meta = fs::metadata(path).ok()?;
    Some(FileStamp {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// Read-decrypt-modify-encrypt-write of the whole blob — still a
/// last-write-wins race under concurrent calls (one caller's entry can be
/// dropped by another's write), the same characteristic every other file
/// under `infra::settings_store` has. What `secret_store` does guarantee is
/// that no reader ever sees a half-written file, so a lost update is the
/// worst case rather than a corrupt store.
pub fn save_api_key(provider_id: &str, api_key: &str) -> Result<(), String> {
    let mut map = load_all();
    map.insert(provider_id.to_string(), api_key.to_string());
    save_all(&map)
}

/// Decrypts and returns `provider_id`'s stored API key, if any — for
/// internal use when actually constructing an `LlmProvider`, never
/// returned from an IPC command.
pub fn get_api_key(provider_id: &str) -> Option<SecretString> {
    let map = load_all();
    map.get(provider_id).map(|key| SecretString::from(key.as_str()))
}

pub fn has_api_key(provider_id: &str) -> bool {
    let Ok(path) = credentials_path() else {
        return false;
    };
    // No file at all means no stored key, and costs one `stat`.
    let Some(current) = stamp(&path) else {
        return false;
    };

    let mut cache = configured_ids().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached, ids)) = cache.as_ref() {
        if *cached == current {
            return ids.contains(provider_id);
        }
    }

    let ids: HashSet<String> = load_all().keys().cloned().collect();
    let answer = ids.contains(provider_id);
    *cache = Some((current, ids));
    answer
}

pub fn delete_api_key(provider_id: &str) -> Result<(), String> {
    let mut map = load_all();
    // `remove` moves the key out of the map, past `Credentials`' own
    // wipe-on-drop — so wipe this one by hand.
    if let Some(mut removed) = map.remove(provider_id) {
        removed.zeroize();
        save_all(&map)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::master_key;
    use secrecy::ExposeSecret;

    /// The reason this cache exists: `has_api_key` is called once per
    /// configured provider on every LLM settings refresh, and each call
    /// used to decrypt the whole blob to answer a boolean.
    #[test]
    fn repeated_has_api_key_decrypts_once() {
        settings_store::test_support::with_temp_home(|| {
            master_key::forget_for_tests();
            save_api_key("openai", "sk-one").unwrap();

            let before = load_count();
            for _ in 0..20 {
                assert!(has_api_key("openai"));
                assert!(!has_api_key("anthropic"));
            }
            assert_eq!(load_count() - before, 1);

            master_key::forget_for_tests();
        });
    }

    #[test]
    fn writes_are_reflected_immediately() {
        settings_store::test_support::with_temp_home(|| {
            master_key::forget_for_tests();

            assert!(!has_api_key("openai"));
            save_api_key("openai", "sk-one").unwrap();
            assert!(has_api_key("openai"));

            save_api_key("anthropic", "sk-two").unwrap();
            assert!(has_api_key("anthropic"));
            assert!(has_api_key("openai"));

            delete_api_key("openai").unwrap();
            assert!(!has_api_key("openai"));
            assert!(has_api_key("anthropic"));

            master_key::forget_for_tests();
        });
    }

    #[test]
    fn keys_round_trip_and_stay_separate() {
        settings_store::test_support::with_temp_home(|| {
            master_key::forget_for_tests();

            save_api_key("openai", "sk-one").unwrap();
            save_api_key("anthropic", "sk-two").unwrap();

            assert_eq!(get_api_key("openai").unwrap().expose_secret(), "sk-one");
            assert_eq!(get_api_key("anthropic").unwrap().expose_secret(), "sk-two");
            assert!(get_api_key("mistral").is_none());

            master_key::forget_for_tests();
        });
    }

    /// A file replaced behind the app's back must not be served from a
    /// stale cache — the `stat` revalidation is what catches that.
    #[test]
    fn an_external_rewrite_is_picked_up() {
        settings_store::test_support::with_temp_home(|| {
            master_key::forget_for_tests();
            save_api_key("openai", "sk-one").unwrap();
            assert!(has_api_key("openai"));

            // Same shape of write, but from "outside": the cache must not
            // keep answering from the previous id set.
            let mut other = Credentials::default();
            other.insert("anthropic".to_string(), "sk-two".to_string());
            let plain = Zeroizing::new(serde_json::to_vec(&other.0).unwrap());
            let path = credentials_path().unwrap();
            let key = master_key::get_or_create().unwrap();
            let blob = secret_store::seal(PURPOSE, &plain, &key).unwrap();
            secret_store::write_atomic_private(&path, &blob).unwrap();

            assert!(has_api_key("anthropic"));
            assert!(!has_api_key("openai"));

            master_key::forget_for_tests();
        });
    }
}
