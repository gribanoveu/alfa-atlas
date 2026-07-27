use aes_gcm::{
    aead::{Aead, OsRng},
    AeadCore, Aes256Gcm, KeyInit, Nonce,
};
use rand::RngCore;
use ssh_key::private::{Ed25519Keypair, KeypairData};
use ssh_key::{LineEnding, PrivateKey};
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::git::{AppKeyStatus, KeyConfig};
use crate::infra::settings_store;

const KEYRING_SERVICE: &str = "com.eugene.docflow";
const KEYRING_USER: &str = "encryption-key";
const KEY_CONFIG_FILE: &str = "key_config.json";
const ENCRYPTED_KEY_FILE: &str = "id_ed25519.enc";
/// File-based fallback for the encryption key when keyring is unavailable/prompted.
const ENC_KEY_FILE: &str = ".enc_key";

fn key_config_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(KEY_CONFIG_FILE))
}

fn encrypted_key_path(relative: &str) -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(relative))
}

fn enc_key_file_path() -> Result<PathBuf, String> {
    let dir = settings_store::settings_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(ENC_KEY_FILE))
}

/// Retrieves or creates a 256-bit AES key. The authoritative store is a file
/// at `~/.docflow/.enc_key` (0o600).  The OS keyring is written as a secondary
/// store but never relied upon for retrieval — macOS silently rejects keychain
/// writes from unsigned binaries (showing no prompt), so the file must be the
/// source of truth.
fn get_or_create_encryption_key() -> Result<[u8; 32], String> {
    let key = file_key_get_or_create()?;

    // Try to mirror the key into the OS keyring as a bonus.  Failure is OK:
    // on unsigned macOS binaries the write silently fails.
    let _ = keyring_set_if_empty(&key);

    Ok(key)
}

/// Returns the keyring entry if it already exists, otherwise writes `key`.
/// Errors are logged but not propagated — the file store is authoritative.
fn keyring_set_if_empty(key: &[u8; 32]) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| {
            eprintln!("[docflow] keyring::Entry::new failed: {e}");
            return format!("{e}");
        })?;

    match entry.get_secret() {
        Ok(secret) if secret.as_slice() == key.as_slice() => {
            // Already in sync — nothing to do.
            Ok(())
        }
        Ok(_) | Err(keyring::Error::NoEntry) => {
            entry.set_secret(key).map_err(|e| {
                eprintln!("[docflow] keyring set_secret failed (non-fatal): {e}");
                format!("{e}")
            })
        }
        Err(e) => {
            eprintln!("[docflow] keyring get_secret failed (non-fatal): {e}");
            Err(format!("{e}"))
        }
    }
}

/// File-based fallback: reads or creates `~/.docflow/.enc_key` (0o600 permissions).
fn file_key_get_or_create() -> Result<[u8; 32], String> {
    let path = enc_key_file_path()?;

    if path.exists() {
        let bytes = fs::read(&path)
            .map_err(|e| format!("failed to read encryption key file: {e}"))?;
        let key: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "stored encryption key has wrong length".to_string())?;
        eprintln!("[docflow] using encryption key from file fallback");
        return Ok(key);
    }

    // Generate and persist a new key.
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)
            .map_err(|e| format!("failed to create settings dir: {e}"))?;
    }

    fs::write(&path, &key)
        .map_err(|e| format!("failed to write encryption key file: {e}"))?;

    // Set restrictive permissions (0o600).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&path, perms);
        }
    }

    eprintln!("[docflow] created new encryption key in file fallback");
    Ok(key)
}

/// AES-256-GCM encrypts `plaintext` with the given key. The nonce is prepended to the output.
fn encrypt_private_key(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| format!("invalid key length: {e}"))?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| format!("encryption failed: {e}"))?;
    let mut result = nonce.to_vec();
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// AES-256-GCM decrypts data produced by `encrypt_private_key` (nonce + ciphertext).
fn decrypt_private_key(encrypted: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, String> {
    if encrypted.len() < 12 {
        return Err("encrypted data too short".to_string());
    }
    let (nonce_bytes, ciphertext) = encrypted.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| format!("invalid key length: {e}"))?;
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("decryption failed: {e}"))
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

/// Loads the key config from `~/.docflow/key_config.json`.
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

/// Saves the key config to `~/.docflow/key_config.json`.
fn save_key_config(config: &KeyConfig) -> Result<(), String> {
    let path = key_config_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("failed to create settings dir: {e}"))?;
    }
    let contents = serde_json::to_string_pretty(config)
        .map_err(|e| format!("failed to serialize key config: {e}"))?;
    fs::write(&path, contents)
        .map_err(|e| format!("failed to write key config: {e}"))?;
    Ok(())
}

/// Ensures the app-managed key exists (generates if not). Returns current status.
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

    // Try decrypting. If decryption fails (e.g. keychain access denied or
    // keyring entry missing), regenerate everything from scratch.
    let mut decryptable = false;
    match get_or_create_encryption_key() {
        Ok(key) => {
            match fs::read(&enc_path) {
                Ok(encrypted) => {
                    if decrypt_private_key(&encrypted, &key).is_ok() {
                        decryptable = true;
                    } else {
                        eprintln!("[docflow] app key decryption failed — regenerating");
                    }
                }
                Err(e) => {
                    eprintln!("[docflow] failed to read encrypted key: {e} — regenerating");
                }
            }
        }
        Err(e) => {
            eprintln!("[docflow] failed to get encryption key from keyring: {e} — regenerating");
        }
    }

    if !decryptable {
        // Delete stale config and encrypted file, then regenerate.
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

/// Generates a new Ed25519 keypair, encrypts it, and stores it.
fn generate_and_store_key(is_imported: bool) -> Result<AppKeyStatus, String> {
    let (private_openssh, public_openssh) = generate_ed25519_keypair()?;
    let encryption_key = get_or_create_encryption_key()?;
    let encrypted = encrypt_private_key(private_openssh.as_bytes(), &encryption_key)?;

    let enc_path = encrypted_key_path(ENCRYPTED_KEY_FILE)?;
    if let Some(dir) = enc_path.parent() {
        fs::create_dir_all(dir)
            .map_err(|e| format!("failed to create settings dir: {e}"))?;
    }
    fs::write(&enc_path, &encrypted)
        .map_err(|e| format!("failed to write encrypted key: {e}"))?;

    let config = KeyConfig {
        public_key: public_openssh.clone(),
        encrypted_private_key_path: ENCRYPTED_KEY_FILE.to_string(),
        is_imported,
    };
    save_key_config(&config)?;

    Ok(AppKeyStatus {
        exists: true,
        public_key: public_openssh,
        private_key_available: true,
        is_imported,
    })
}

/// Imports an existing private key file, encrypts it, and stores it as the app-managed key.
pub fn import_key_file(source_path: &Path) -> Result<AppKeyStatus, String> {
    let private_key_content =
        fs::read_to_string(source_path).map_err(|e| format!("failed to read key file: {e}"))?;

    let parsed = PrivateKey::from_openssh(&private_key_content)
        .map_err(|e| format!("failed to parse SSH private key: {e}"))?;

    let public_openssh = parsed
        .public_key()
        .to_openssh()
        .map_err(|e| format!("failed to serialize public key: {e}"))?;

    let encryption_key = get_or_create_encryption_key()?;
    let encrypted = encrypt_private_key(private_key_content.as_bytes(), &encryption_key)?;

    let enc_path = encrypted_key_path(ENCRYPTED_KEY_FILE)?;
    if let Some(dir) = enc_path.parent() {
        fs::create_dir_all(dir)
            .map_err(|e| format!("failed to create settings dir: {e}"))?;
    }
    fs::write(&enc_path, &encrypted)
        .map_err(|e| format!("failed to write encrypted key: {e}"))?;

    let config = KeyConfig {
        public_key: public_openssh.clone(),
        encrypted_private_key_path: ENCRYPTED_KEY_FILE.to_string(),
        is_imported: true,
    };
    save_key_config(&config)?;

    Ok(AppKeyStatus {
        exists: true,
        public_key: public_openssh,
        private_key_available: true,
        is_imported: true,
    })
}

/// Decrypts and returns the app-managed private key as an OpenSSH string.
pub fn get_decrypted_private_key() -> Option<String> {
    let config = match load_key_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[docflow] get_decrypted_private_key: failed to load key config: {e}");
            return None;
        }
    };
    if config.encrypted_private_key_path.is_empty() {
        return None;
    }
    let enc_path = match encrypted_key_path(&config.encrypted_private_key_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[docflow] get_decrypted_private_key: failed to resolve encrypted key path: {e}");
            return None;
        }
    };
    let encrypted = match fs::read(&enc_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("[docflow] get_decrypted_private_key: failed to read encrypted key at {}: {e}", enc_path.display());
            return None;
        }
    };
    let key = match get_or_create_encryption_key() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("[docflow] get_decrypted_private_key: encryption key unavailable: {e}");
            return None;
        }
    };
    match decrypt_private_key(&encrypted, &key) {
        Ok(plain) => match String::from_utf8(plain) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("[docflow] get_decrypted_private_key: decrypted data is not valid UTF-8: {e}");
                None
            }
        },
        Err(e) => {
            eprintln!("[docflow] get_decrypted_private_key: decryption failed: {e}");
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
    fn encrypt_decrypt_roundtrip() {
        let key = [42u8; 32];
        let plaintext = b"-----BEGIN OPENSSH PRIVATE KEY-----\ntest\n-----END OPENSSH PRIVATE KEY-----\n";
        let encrypted = encrypt_private_key(plaintext, &key).unwrap();
        assert!(encrypted.len() > 12);
        let decrypted = decrypt_private_key(&encrypted, &key).unwrap();
        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn decrypt_wrong_key_fails() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let encrypted = encrypt_private_key(b"secret", &key1).unwrap();
        assert!(decrypt_private_key(&encrypted, &key2).is_err());
    }

    #[test]
    fn decrypt_short_data_fails() {
        let key = [1u8; 32];
        assert!(decrypt_private_key(b"short", &key).is_err());
    }

    #[test]
    fn generate_keypair_produces_valid_openssh() {
        let (private, public) = generate_ed25519_keypair().unwrap();
        assert!(private.contains("BEGIN OPENSSH PRIVATE KEY"));
        assert!(private.contains("END OPENSSH PRIVATE KEY"));
        assert!(public.starts_with("ssh-ed25519 "));
        // Verify the private key is parseable.
        PrivateKey::from_openssh(&private).unwrap();
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
