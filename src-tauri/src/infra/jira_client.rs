//! The one place `gouqi` is spoken to. Builds a blocking Jira client out of
//! resolved settings + the stored token and answers the only question the
//! app asks today: who does this token belong to?
//!
//! Why not `infra::http_agent`: that builds a `ureq::Agent`, and `gouqi` is
//! built on `reqwest`. The corporate-CA override therefore has to be
//! re-expressed against reqwest's own builder here — same semantics as
//! `http_agent::build_agent` (the PEM bundle *replaces* the public roots
//! rather than adding to them), just a different TLS stack.
//!
//! Blocking on purpose: every caller already runs inside `spawn_blocking`,
//! matching how the LLM and embedding clients are driven. Pulling gouqi's
//! `async` feature would add a second tokio runtime for no gain.

use gouqi::{Credentials, Error as GouqiError, Jira};
use reqwest::blocking::Client;

use crate::domain::jira::{JiraError, JiraSettings, JiraUser, JiraWebLink};

/// `trusted_cert_pem` replaces the built-in roots entirely when present —
/// an instance either needs its own CA trusted or it doesn't, and mixing
/// the two only widens what this client would accept.
fn http_client(trusted_cert_pem: Option<&str>) -> Result<Client, JiraError> {
    let mut builder = Client::builder();
    if let Some(pem) = trusted_cert_pem {
        // Unlike `Certificate::from_pem`, the bundle form parses every
        // certificate in the blob — a corporate CA is commonly handed out
        // as a root plus one or more intermediates concatenated together.
        let certs = reqwest::Certificate::from_pem_bundle(pem.as_bytes())
            .map_err(|e| JiraError::Tls(e.to_string()))?;
        if certs.is_empty() {
            return Err(JiraError::Tls("no PEM-encoded certificate found".to_string()));
        }
        builder = builder.tls_built_in_root_certs(false);
        for cert in certs {
            builder = builder.add_root_certificate(cert);
        }
    }
    builder.build().map_err(|e| JiraError::Tls(e.to_string()))
}

/// Builds a client for `settings` (already merged with the build preset by
/// `services::jira_config::resolve`), authenticating with `token`.
pub fn connect(settings: &JiraSettings, token: String) -> Result<Jira, JiraError> {
    let base_url = settings.base_url.trim();
    if base_url.is_empty() {
        return Err(JiraError::NotConfigured);
    }
    let cert = settings
        .trusted_cert_pem
        .as_deref()
        .map(str::trim)
        .filter(|pem| !pem.is_empty());

    Jira::from_client(base_url.to_string(), Credentials::Bearer(token), http_client(cert)?)
        .map_err(|e| JiraError::InvalidBaseUrl(e.to_string()))
}

/// `GET /rest/api/latest/myself` — the account behind the token.
///
/// Not `Jira::session()`: that hits `/rest/auth/1/session`, which carries
/// nothing but a login name and answers for the *cookie* session rather
/// than for a Bearer token. `/myself` returns the display name, email and
/// avatar this feature actually shows.
pub fn current_user(jira: &Jira) -> Result<JiraUser, JiraError> {
    let user: gouqi::User = jira.get("api", "/myself").map_err(map_error)?;

    Ok(JiraUser {
        display_name: user.display_name,
        email_address: user.email_address,
        // Cloud fills `accountId`; Server/DC leaves it empty and fills
        // `name` (the login) instead, so fall back rather than show nothing.
        account_id: user.account_id.or(user.name).or(user.key),
        active: user.active,
    })
}

/// `POST /rest/api/latest/issue/{key}/remotelink` — attaches one Web Link.
///
/// Idempotent by `globalId` (see `JiraWebLink::to_payload`): attaching the
/// same URL again updates that link instead of adding a duplicate, so a
/// retry after a partial failure is safe.
pub fn attach_web_link(jira: &Jira, issue_key: &str, link: &JiraWebLink) -> Result<(), JiraError> {
    // The response body carries the created link's id, which nothing here
    // needs — `IgnoredAny` avoids inventing a type to throw away.
    jira.post::<serde::de::IgnoredAny, _>(
        "api",
        &format!("/issue/{issue_key}/remotelink"),
        link.to_payload(),
    )
    .map(|_| ())
    .map_err(map_error)
}

/// Collapses gouqi's error surface into the two cases the UI distinguishes:
/// "the token is wrong" and "everything else". 401/403 arrive both as the
/// dedicated `Unauthorized` variant and as a `Fault` depending on how the
/// instance answers, so both spellings are folded together.
fn map_error(error: GouqiError) -> JiraError {
    match error {
        GouqiError::Unauthorized => JiraError::Unauthorized,
        GouqiError::Fault { code, .. } if code.as_u16() == 401 || code.as_u16() == 403 => {
            JiraError::Unauthorized
        }
        other => JiraError::Request(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> JiraSettings {
        JiraSettings {
            base_url: "https://jira.example.com".to_string(),
            trusted_cert_pem: None,
        }
    }

    #[test]
    fn an_empty_base_url_is_reported_as_unconfigured() {
        let mut settings = settings();
        settings.base_url = "   ".to_string();
        let err = connect(&settings, "t".to_string()).unwrap_err();
        assert!(matches!(err, JiraError::NotConfigured));
    }

    #[test]
    fn a_malformed_trusted_certificate_is_a_tls_error() {
        let mut settings = settings();
        settings.trusted_cert_pem = Some("not a pem".to_string());
        let err = connect(&settings, "t".to_string()).unwrap_err();
        assert!(matches!(err, JiraError::Tls(_)), "unexpected error: {err}");
    }

    #[test]
    fn a_blank_certificate_field_is_not_treated_as_an_override() {
        let mut settings = settings();
        settings.trusted_cert_pem = Some("   \n".to_string());
        assert!(connect(&settings, "t".to_string()).is_ok());
    }

    #[test]
    fn connects_with_a_valid_base_url() {
        assert!(connect(&settings(), "t".to_string()).is_ok());
    }

    /// Диагностика живого инстанса — не часть обычного прогона, запускается
    /// вручную, когда «в панели Jira ошибка, а curl в тот же адрес ходит».
    /// Проходит ровно тем же кодом, что и приложение, и потому ловит именно
    /// эту разницу: curl на macOS доверяет системному хранилищу, а rustls —
    /// только публичным корням webpki плюс тому, что передали явно. Поэтому
    /// проба идёт дважды и печатает, какой вариант доверия прошёл.
    ///
    /// ```text
    /// JIRA_URL=https://jira.host JIRA_TOKEN=... \
    ///   cargo test --lib jira_live_probe -- --ignored --nocapture
    /// ```
    /// Ни адрес, ни токен в коде не хранятся: адрес корпоративный, токен
    /// личный. Сертификат берётся из `JIRA_CERT_FILE`, если задан.
    #[test]
    #[ignore = "диагностика: нужен доступ к серверу, параметры в JIRA_URL/JIRA_TOKEN"]
    fn jira_live_probe() {
        let base_url = std::env::var("JIRA_URL").expect("задайте JIRA_URL=https://jira.host");
        let token = std::env::var("JIRA_TOKEN").expect("задайте JIRA_TOKEN=<personal access token>");
        let cert = std::env::var("JIRA_CERT_FILE")
            .ok()
            .map(|path| std::fs::read_to_string(path).expect("JIRA_CERT_FILE не читается"));

        for (name, pem) in [
            ("переданный корневой сертификат", cert.as_deref()),
            ("только публичные корни webpki", None),
        ] {
            if name.starts_with("переданный") && pem.is_none() {
                println!("\n=== вариант: {name} — пропущен, JIRA_CERT_FILE не задан");
                continue;
            }
            println!("\n=== вариант: {name} ===");
            let settings = JiraSettings {
                base_url: base_url.clone(),
                trusted_cert_pem: pem.map(str::to_string),
            };
            let outcome = connect(&settings, token.clone()).and_then(|jira| current_user(&jira));
            match outcome {
                Ok(user) => println!(
                    "  -> OK: {} <{}>, учётная запись {}, активна: {}",
                    user.display_name,
                    user.email_address.as_deref().unwrap_or("—"),
                    user.account_id.as_deref().unwrap_or("—"),
                    user.active,
                ),
                Err(e) => println!("  -> ОШИБКА: {e}"),
            }
        }
    }
}
