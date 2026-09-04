//! The app-managed SSH key used for git over SSH: generation, import, and
//! its sealed storage at `~/.atlas/id_ed25519.enc`.
//!
//! The master key this is sealed under lives in `infra::master_key` (the OS
//! keychain, with a file fallback), and the blob format is
//! `infra::secret_store`'s — this module owns neither, it only decides what
//! an SSH key is and when to make one.

use aes_gcm::aead::OsRng;
use ssh_key::private::{Ed25519Keypair, KeypairData};
use ssh_key::{LineEnding, PrivateKey};
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::git::{AppKeyStatus, KeyConfig};
use crate::infra::secret_store::{self, SecretPurpose};
use crate::infra::settings_store;

const KEY_CONFIG_FILE: &str = "key_config.json";
const ENCRYPTED_KEY_FILE: &str = "id_ed25519.enc";
const PURPOSE: SecretPurpose = SecretPurpose::SshPrivateKey;

fn key_config_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(KEY_CONFIG_FILE))
}

fn encrypted_key_path(relative: &str) -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(relative))
}

/// Generates an Ed25519 SSH key pair. Returns (private_key_openssh, public_key_openssh).
fn generate_ed25519_keypair() -> Result<(String, String), String> {
    let ed25519_keypair = Ed25519Keypair::random(&mut OsRng);
    let keypair_data = KeypairData::Ed25519(ed25519_keypair);
    let private = PrivateKey::new(keypair_data, "")
        .map_err(|e| format!("failed to create private key: {e}"))?;

    let private_openssh = private
        .to_openssh(LineEnding::LF)
        .map_err(|e| format!("failed to serialize private key: {e}"))?;

    let public_openssh = private
        .public_key()
        .to_openssh()
        .map_err(|e| format!("failed to serialize public key: {e}"))?;

    Ok((private_openssh.to_string(), public_openssh))
}

/// Loads the key config from `~/.atlas/key_config.json`.
/// Returns `KeyConfig::default()` if the file does not exist.
pub fn load_key_config() -> Result<KeyConfig, String> {
    let path = key_config_path()?;
    if !path.exists() {
        return Ok(KeyConfig::default());
    }
    let contents =
        fs::read_to_string(&path).map_err(|e| format!("failed to read key config: {e}"))?;
    let config: KeyConfig =
        serde_json::from_str(&contents).map_err(|e| format!("failed to parse key config: {e}"))?;
    Ok(config)
}

/// Saves the key config to `~/.atlas/key_config.json`.
fn save_key_config(config: &KeyConfig) -> Result<(), String> {
    let path = key_config_path()?;
    let contents = serde_json::to_string_pretty(config)
        .map_err(|e| format!("failed to serialize key config: {e}"))?;
    // Not a secret (it holds the *public* key and a filename), but it sits
    // in the same directory and benefits from the same atomic replace.
    secret_store::write_atomic_private(&path, contents.as_bytes())
}

/// Ensures the app-managed key exists (generates if not). Returns current status.
///
/// A key that cannot be decrypted is replaced, not repaired — the private
/// half is unrecoverable at that point, and the user's next step either way
/// is to register the new public key with their git host.
pub fn ensure_app_key_exists() -> Result<AppKeyStatus, String> {
    let config = load_key_config()?;

    if config.encrypted_private_key_path.is_empty() {
        return generate_and_store_key(false);
    }

    let enc_path = encrypted_key_path(&config.encrypted_private_key_path)?;
    if !enc_path.exists() {
        // Config exists but encrypted file is missing — regenerate.
        return generate_and_store_key(false);
    }

    if secret_store::read_secret_file(&enc_path, PURPOSE).is_none() {
        eprintln!("[alfa-atlas] app SSH key could not be decrypted — regenerating");
        let _ = fs::remove_file(&enc_path);
        let _ = fs::remove_file(key_config_path().unwrap_or_default());
        return generate_and_store_key(false);
    }

    Ok(AppKeyStatus {
        exists: true,
        public_key: config.public_key.clone(),
        private_key_available: true,
        is_imported: config.is_imported,
    })
}

/// Seals `private_openssh` at `id_ed25519.enc` and records `public_openssh`
/// in the key config.
fn store_keypair(
    private_openssh: &str,
    public_openssh: String,
    is_imported: bool,
) -> Result<AppKeyStatus, String> {
    let enc_path = encrypted_key_path(ENCRYPTED_KEY_FILE)?;
    secret_store::write_secret_file(&enc_path, PURPOSE, private_openssh.as_bytes())?;

    save_key_config(&KeyConfig {
        public_key: public_openssh.clone(),
        encrypted_private_key_path: ENCRYPTED_KEY_FILE.to_string(),
        is_imported,
    })?;

    Ok(AppKeyStatus {
        exists: true,
        public_key: public_openssh,
        private_key_available: true,
        is_imported,
    })
}

/// Generates a new Ed25519 keypair, seals it, and stores it.
fn generate_and_store_key(is_imported: bool) -> Result<AppKeyStatus, String> {
    let (private_openssh, public_openssh) = generate_ed25519_keypair()?;
    store_keypair(&private_openssh, public_openssh, is_imported)
}

/// Imports an existing private key file, seals it, and stores it as the app-managed key.
pub fn import_key_file(source_path: &Path) -> Result<AppKeyStatus, String> {
    let private_key_content =
        fs::read_to_string(source_path).map_err(|e| format!("failed to read key file: {e}"))?;

    let parsed = PrivateKey::from_openssh(&private_key_content)
        .map_err(|e| format!("failed to parse SSH private key: {e}"))?;

    let public_openssh = parsed
        .public_key()
        .to_openssh()
        .map_err(|e| format!("failed to serialize public key: {e}"))?;

    store_keypair(&private_key_content, public_openssh, true)
}

/// Decrypts and returns the app-managed private key as an OpenSSH string.
pub fn get_decrypted_private_key() -> Option<String> {
    let config = match load_key_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[alfa-atlas] get_decrypted_private_key: failed to load key config: {e}");
            return None;
        }
    };
    if config.encrypted_private_key_path.is_empty() {
        return None;
    }
    let enc_path = match encrypted_key_path(&config.encrypted_private_key_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[alfa-atlas] get_decrypted_private_key: failed to resolve encrypted key path: {e}");
            return None;
        }
    };
    let plain = secret_store::read_secret_file(&enc_path, PURPOSE)?;
    match String::from_utf8(plain) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("[alfa-atlas] get_decrypted_private_key: decrypted data is not valid UTF-8: {e}");
            None
        }
    }
}

/// Generates a new key (replaces existing one) and returns the status.
/// This is the public API exposed via the IPC command for explicit key generation.
pub fn generate_and_store_key_app() -> Result<AppKeyStatus, String> {
    generate_and_store_key(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_keypair_produces_valid_openssh() {
        let (private, public) = generate_ed25519_keypair().unwrap();
        assert!(private.contains("BEGIN OPENSSH PRIVATE KEY"));
        assert!(private.contains("END OPENSSH PRIVATE KEY"));
        assert!(public.starts_with("ssh-ed25519 "));
        // Verify the private key is parseable.
        PrivateKey::from_openssh(&private).unwrap();
    }

    /// End-to-end through the real storage layer: generate, then read the
    /// key back the way `git_ops` does.
    #[test]
    fn generated_key_round_trips_through_storage() {
        settings_store::test_support::with_temp_home(|| {
            crate::infra::master_key::forget_for_tests();

            let status = generate_and_store_key(false).unwrap();
            assert!(status.exists);
            assert!(status.public_key.starts_with("ssh-ed25519 "));

            let private = get_decrypted_private_key().expect("key should be readable");
            PrivateKey::from_openssh(&private).unwrap();

            // A second call must not replace a healthy key.
            let again = ensure_app_key_exists().unwrap();
            assert_eq!(again.public_key, status.public_key);

            crate::infra::master_key::forget_for_tests();
        });
    }

    /// A corrupt blob is not silently served as a key — it is replaced.
    #[test]
    fn unreadable_key_is_regenerated() {
        settings_store::test_support::with_temp_home(|| {
            crate::infra::master_key::forget_for_tests();

            let first = generate_and_store_key(false).unwrap();
            let enc_path = encrypted_key_path(ENCRYPTED_KEY_FILE).unwrap();
            fs::write(&enc_path, b"not a sealed blob at all").unwrap();

            let second = ensure_app_key_exists().unwrap();
            assert!(second.exists);
            assert_ne!(second.public_key, first.public_key);
            assert!(get_decrypted_private_key().is_some());

            crate::infra::master_key::forget_for_tests();
        });
    }

    #[test]
    fn app_key_status_serialization() {
        let status = AppKeyStatus {
            exists: true,
            public_key: "ssh-ed25519 AAAAC3...".into(),
            private_key_available: true,
            is_imported: false,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""exists":true"#));
        assert!(json.contains(r#""privateKeyAvailable":true"#));
        assert!(json.contains(r#""isImported":false"#));

        let parsed: AppKeyStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status);
    }

    #[test]
    fn key_config_serialization() {
        let config = KeyConfig {
            public_key: "ssh-ed25519 AAAAC3...".into(),
            encrypted_private_key_path: "id_ed25519.enc".into(),
            is_imported: false,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""encryptedPrivateKeyPath":"id_ed25519.enc""#));

        let parsed: KeyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn key_config_default_is_empty() {
        let config = KeyConfig::default();
        assert!(config.public_key.is_empty());
        assert!(config.encrypted_private_key_path.is_empty());
        assert!(!config.is_imported);
    }
}
