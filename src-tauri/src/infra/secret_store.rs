//! The on-disk format for every secret under `~/.atlas`, and the only way
//! those files are written.
//!
//! ## Blob layout (v1)
//!
//! ```text
//! "ATLS" | 0x01 | purpose_len: u8 | purpose | nonce (12) | ciphertext+tag
//! \_______________ header, also the AES-GCM AAD ________/
//! ```
//!
//! Binding the header — and with it the `purpose` — as associated data is
//! the point of the format. The previous layout was a bare `nonce ||
//! ciphertext` under one app-wide key, so the four blobs were structurally
//! interchangeable: dropping `jira_credentials.enc` over
//! `llm_credentials.enc` produced a file that decrypted perfectly and fed a
//! Jira token to an LLM provider as its API key. A purpose in the AAD makes
//! that a decryption failure instead. The version byte is what lets a later
//! format (an Argon2-wrapped DEK, say) land without guessing.
//!
//! ## Reading older files
//!
//! `open` accepts the legacy headerless layout so an upgrade does not
//! orphan anyone's stored tokens, and `read_secret_file` rewrites such a
//! blob in v1 on the spot, so the legacy path drains rather than lingering
//! indefinitely.

use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::{
    aead::{Aead, OsRng, Payload},
    AeadCore, Aes256Gcm, KeyInit, Nonce,
};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

use crate::infra::master_key::{self, KEY_LEN};
use crate::infra::settings_store;

/// A stable, non-reversible fingerprint of a secret, for use as a cache key.
///
/// The provider caches need to answer "is this the same key as last time?"
/// on every request. Keeping the key itself to compare against means a
/// second plaintext copy of a credential living for the whole process; and
/// `SecretString` deliberately implements no `PartialEq`, precisely to
/// discourage that. A digest answers the same question exactly while
/// retaining nothing usable.
pub(crate) fn fingerprint(secret: Option<&SecretString>) -> Option<[u8; 32]> {
    secret.map(|s| *blake3::hash(s.expose_secret().as_bytes()).as_bytes())
}

const MAGIC: &[u8; 4] = b"ATLS";
const VERSION: u8 = 1;
const NONCE_LEN: usize = 12;

/// What a blob is *for*. Sealed into the AAD, so a blob can only ever be
/// opened as the kind of secret it was written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecretPurpose {
    LlmCredentials,
    JiraToken,
    EmbeddingApiKey,
    SshPrivateKey,
    CalendarPassword,
}

impl SecretPurpose {
    /// Stable wire tag — never change these, they are part of the format.
    fn tag(self) -> &'static [u8] {
        match self {
            Self::LlmCredentials => b"llm-credentials",
            Self::JiraToken => b"jira-token",
            Self::EmbeddingApiKey => b"embedding-api-key",
            Self::SshPrivateKey => b"ssh-private-key",
            Self::CalendarPassword => b"calendar-password",
        }
    }
}

fn header_for(purpose: SecretPurpose) -> Vec<u8> {
    let tag = purpose.tag();
    let mut header = Vec::with_capacity(6 + tag.len());
    header.extend_from_slice(MAGIC);
    header.push(VERSION);
    header.push(tag.len() as u8);
    header.extend_from_slice(tag);
    header
}

/// Encrypts `plaintext` as a v1 blob bound to `purpose`.
pub(crate) fn seal(
    purpose: SecretPurpose,
    plaintext: &[u8],
    key: &[u8; KEY_LEN],
) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("invalid key length: {e}"))?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let header = header_for(purpose);

    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: &header,
            },
        )
        .map_err(|e| format!("encryption failed: {e}"))?;

    let mut blob = header;
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Decrypts a blob written by `seal`, or a legacy headerless one.
///
/// A v1 blob for a *different* purpose fails here — that is the guarantee
/// the AAD buys. Note the deliberate fall-through: a legacy blob whose
/// random nonce happens to begin with `ATLS` (1 in 2^32) would parse as a
/// v1 header and fail, so a failed v1 open retries the legacy layout rather
/// than giving up.
pub(crate) fn open(
    purpose: SecretPurpose,
    blob: &[u8],
    key: &[u8; KEY_LEN],
) -> Result<Zeroizing<Vec<u8>>, String> {
    if blob.starts_with(MAGIC) {
        if let Ok(plain) = open_v1(purpose, blob, key) {
            return Ok(plain);
        }
    }
    open_legacy(blob, key)
}

fn open_v1(
    purpose: SecretPurpose,
    blob: &[u8],
    key: &[u8; KEY_LEN],
) -> Result<Zeroizing<Vec<u8>>, String> {
    if blob.len() < 6 {
        return Err("sealed blob is truncated".to_string());
    }
    if blob[4] != VERSION {
        return Err(format!("unsupported sealed-blob version {}", blob[4]));
    }
    let tag_len = blob[5] as usize;
    let header_len = 6 + tag_len;
    if blob.len() < header_len + NONCE_LEN {
        return Err("sealed blob is truncated".to_string());
    }
    if &blob[6..header_len] != purpose.tag() {
        return Err("sealed blob was written for a different purpose".to_string());
    }

    let (header, rest) = blob.split_at(header_len);
    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("invalid key length: {e}"))?;
    cipher
        .decrypt(
            Nonce::from_slice(nonce_bytes),
            Payload {
                msg: ciphertext,
                aad: header,
            },
        )
        .map(Zeroizing::new)
        .map_err(|e| format!("decryption failed: {e}"))
}

/// Pre-v1 layout: `nonce || ciphertext`, no AAD, no purpose binding.
fn open_legacy(blob: &[u8], key: &[u8; KEY_LEN]) -> Result<Zeroizing<Vec<u8>>, String> {
    if blob.len() < NONCE_LEN {
        return Err("encrypted data too short".to_string());
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("invalid key length: {e}"))?;
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map(Zeroizing::new)
        .map_err(|e| format!("decryption failed: {e}"))
}

/// Writes `bytes` so a reader sees either the whole old file or the whole
/// new one, never a half-written blob: write a sibling temp file, narrow it
/// to `0o600`, then rename over the target.
///
/// `fs::rename` replaces an existing destination on both unix and Windows,
/// and the temp file carries the final permissions before it is ever
/// visible under the real name.
pub(crate) fn write_atomic_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    settings_store::ensure_settings_dir().map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("failed to create settings dir: {e}"))?;
    }

    let tmp = temp_sibling(path);
    fs::write(&tmp, bytes).map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&tmp) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&tmp, perms);
        }
    }

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("failed to replace {}: {e}", path.display()));
    }
    Ok(())
}

fn temp_sibling(path: &Path) -> PathBuf {
    let mut suffix = [0u8; 8];
    OsRng.fill_bytes(&mut suffix);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "secret".to_string());
    let hex: String = suffix.iter().map(|b| format!("{b:02x}")).collect();
    path.with_file_name(format!(".{name}.{hex}.tmp"))
}

/// Seals `plaintext` under the app master key and writes it to `path`.
pub(crate) fn write_secret_file(
    path: &Path,
    purpose: SecretPurpose,
    plaintext: &[u8],
) -> Result<(), String> {
    let key = master_key::get_or_create()?;
    let blob = seal(purpose, plaintext, &key)?;
    write_atomic_private(path, &blob)
}

/// Reads and opens `path`, or `None` for any reason it cannot be produced
/// — missing file, unreadable, master key gone, blob for another purpose.
///
/// Every caller's next step is identical regardless of which of those it
/// was ("no credential is configured"), and distinguishing them here would
/// only invite leaking the reason into an error message shown to a user.
/// A legacy blob is re-sealed in v1 as a side effect; that rewrite is
/// best-effort and a failure never denies the caller the secret it asked
/// for.
pub(crate) fn read_secret_file(path: &Path, purpose: SecretPurpose) -> Option<Zeroizing<Vec<u8>>> {
    let blob = fs::read(path).ok()?;
    let key = master_key::get_or_create().ok()?;
    let plain = open(purpose, &blob, &key).ok()?;

    if !blob.starts_with(MAGIC) {
        match seal(purpose, &plain, &key).and_then(|b| write_atomic_private(path, &b)) {
            Ok(()) => eprintln!("[alfa-atlas] re-sealed {} in the v1 format", path.display()),
            Err(e) => eprintln!("[alfa-atlas] could not re-seal {}: {e}", path.display()),
        }
    }

    Some(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; KEY_LEN] = [42u8; KEY_LEN];

    /// The reason secrets are wrapped at all: a `SecretString` that ends up
    /// inside a formatted log line, a panic message or a `Debug`-derived
    /// error must not carry the credential with it.
    #[test]
    fn a_wrapped_secret_does_not_print_itself() {
        let secret = SecretString::from("sk-live-abcdef123456");
        assert!(!format!("{secret:?}").contains("sk-live"));

        #[derive(Debug)]
        #[allow(dead_code)]
        struct ProviderConfig {
            base_url: String,
            api_key: SecretString,
        }
        let config = ProviderConfig {
            base_url: "https://api.example.com".to_string(),
            api_key: SecretString::from("sk-live-abcdef123456"),
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("sk-live"), "leaked through Debug: {rendered}");
        assert!(rendered.contains("api.example.com"), "should still be useful");
    }

    #[test]
    fn fingerprint_identifies_without_revealing() {
        let key = SecretString::from("sk-live-abcdef123456");
        let same = SecretString::from("sk-live-abcdef123456");
        let other = SecretString::from("sk-live-something-else");

        assert_eq!(fingerprint(Some(&key)), fingerprint(Some(&same)));
        assert_ne!(fingerprint(Some(&key)), fingerprint(Some(&other)));
        assert_eq!(fingerprint(None), None);
        assert_ne!(fingerprint(Some(&key)), None);

        // Nothing of the key survives in what the cache retains.
        let digest = fingerprint(Some(&key)).unwrap();
        assert!(!digest.windows(3).any(|w| w == b"sk-"));
    }

    #[test]
    fn seal_open_round_trip() {
        let blob = seal(SecretPurpose::JiraToken, b"a-jira-token", &KEY).unwrap();
        assert!(blob.starts_with(MAGIC));
        let plain = open(SecretPurpose::JiraToken, &blob, &KEY).unwrap();
        assert_eq!(plain.as_slice(), b"a-jira-token");
    }

    #[test]
    fn a_blob_cannot_be_opened_as_another_purpose() {
        // The whole point of the AAD: `jira_credentials.enc` copied over
        // `llm_credentials.enc` must not decrypt.
        let blob = seal(SecretPurpose::JiraToken, b"a-jira-token", &KEY).unwrap();
        assert!(open(SecretPurpose::LlmCredentials, &blob, &KEY).is_err());
        assert!(open(SecretPurpose::EmbeddingApiKey, &blob, &KEY).is_err());
        assert!(open(SecretPurpose::SshPrivateKey, &blob, &KEY).is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let blob = seal(SecretPurpose::LlmCredentials, b"secret", &KEY).unwrap();
        assert!(open(SecretPurpose::LlmCredentials, &blob, &[7u8; KEY_LEN]).is_err());
    }

    #[test]
    fn tampering_with_the_header_fails() {
        let mut blob = seal(SecretPurpose::LlmCredentials, b"secret", &KEY).unwrap();
        blob[4] = 9; // version byte
        assert!(open(SecretPurpose::LlmCredentials, &blob, &KEY).is_err());
    }

    #[test]
    fn truncated_blobs_fail_cleanly() {
        let blob = seal(SecretPurpose::JiraToken, b"token", &KEY).unwrap();
        for cut in [0, 1, 5, 6, 10, blob.len() - 1] {
            assert!(open(SecretPurpose::JiraToken, &blob[..cut], &KEY).is_err());
        }
    }

    /// Byte-for-byte reproduction of the pre-v1 writer, so the compatibility
    /// path is tested against the real old layout rather than a guess.
    fn seal_legacy(plaintext: &[u8], key: &[u8; KEY_LEN]) -> Vec<u8> {
        let cipher = Aes256Gcm::new_from_slice(key).unwrap();
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher.encrypt(&nonce, plaintext).unwrap();
        let mut out = nonce.to_vec();
        out.extend_from_slice(&ciphertext);
        out
    }

    #[test]
    fn legacy_blobs_still_open() {
        let legacy = seal_legacy(b"old-token", &KEY);
        assert!(!legacy.starts_with(MAGIC));
        // Any purpose opens a legacy blob — it carries none, and refusing
        // would lock users out of tokens stored by an older build.
        let plain = open(SecretPurpose::JiraToken, &legacy, &KEY).unwrap();
        assert_eq!(plain.as_slice(), b"old-token");
    }

    #[test]
    fn a_legacy_blob_whose_nonce_starts_with_the_magic_still_opens() {
        // 1-in-2^32 on real data, but the fall-through that handles it is
        // cheap and its absence would be a rare, unreproducible data loss.
        let cipher = Aes256Gcm::new_from_slice(&KEY).unwrap();
        let mut nonce_bytes = [0u8; NONCE_LEN];
        nonce_bytes[..4].copy_from_slice(MAGIC);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let mut legacy = nonce_bytes.to_vec();
        legacy.extend_from_slice(&cipher.encrypt(nonce, b"old-token".as_ref()).unwrap());

        assert_eq!(
            open(SecretPurpose::JiraToken, &legacy, &KEY).unwrap().as_slice(),
            b"old-token"
        );
    }

    #[test]
    fn write_is_atomic_and_private_and_leaves_no_temp_file() {
        settings_store::test_support::with_temp_home(|| {
            let dir = settings_store::ensure_settings_dir().unwrap();
            let path = dir.join("probe.enc");

            write_atomic_private(&path, b"first").unwrap();
            assert_eq!(fs::read(&path).unwrap(), b"first");
            write_atomic_private(&path, b"second-and-longer").unwrap();
            assert_eq!(fs::read(&path).unwrap(), b"second-and-longer");

            let strays: Vec<_> = fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".tmp"))
                .collect();
            assert!(strays.is_empty(), "left temp files behind: {strays:?}");

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
                assert_eq!(
                    fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
        });
    }

    #[test]
    fn reading_a_legacy_file_upgrades_it_in_place() {
        settings_store::test_support::with_temp_home(|| {
            master_key::forget_for_tests();
            let key = *master_key::get_or_create().unwrap();
            let dir = settings_store::ensure_settings_dir().unwrap();
            let path = dir.join("legacy.enc");
            fs::write(&path, seal_legacy(b"old-token", &key)).unwrap();

            let plain = read_secret_file(&path, SecretPurpose::JiraToken).unwrap();
            assert_eq!(plain.as_slice(), b"old-token");

            let on_disk = fs::read(&path).unwrap();
            assert!(on_disk.starts_with(MAGIC), "should have been re-sealed");
            assert_eq!(
                read_secret_file(&path, SecretPurpose::JiraToken).unwrap().as_slice(),
                b"old-token"
            );

            master_key::forget_for_tests();
        });
    }

    #[test]
    fn missing_file_reads_as_none() {
        settings_store::test_support::with_temp_home(|| {
            let dir = settings_store::ensure_settings_dir().unwrap();
            assert!(read_secret_file(&dir.join("nope.enc"), SecretPurpose::JiraToken).is_none());
        });
    }
}
