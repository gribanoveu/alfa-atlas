//! Jira integration: connection settings and the identity of whoever the
//! stored token belongs to. Pure data — no HTTP, no `gouqi` types leak in
//! here (the client lives in `infra::jira_client`).
//!
//! Authentication is a Personal Access Token in an `Authorization: Bearer`
//! header — the Jira Server / Data Center scheme. Cloud's "email + API
//! token as HTTP Basic" is deliberately not offered.
//!
//! The token itself is deliberately *not* part of `JiraSettings`: settings
//! round-trip through `~/.atlas/settings.json` in plaintext, so the token
//! lives encrypted in `infra::jira_credentials_store` exactly like the LLM
//! and embedding keys do.
//!
//! Two layers, exactly like the LLM providers: a build-time `JiraPreset`
//! from the `jira` section of `assets/llm/system_providers.yaml`, and the
//! user's `JiraSettings` on top of it (see `services::jira_config::resolve`).
//! That is what lets a corporate build ship its instance address and CA
//! certificate without anyone pasting them by hand, and without a `.rs` edit.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Build-time defaults, from the manifest's `jira` section. Every field is
/// optional: a manifest with no `jira` section at all is valid and simply
/// means the user configures everything themselves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JiraPreset {
    pub base_url: Option<String>,
    pub trusted_cert_pem: Option<String>,
}

/// The user layer. Empty fields fall back to `JiraPreset`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct JiraSettings {
    /// Instance root, e.g. `https://jira.example.com` — *not* a `/rest/...`
    /// path; the client appends `rest/api/latest/...` itself.
    pub base_url: String,
    /// PEM bundle whose certificates *replace* the public trust roots for
    /// Jira requests — the same escape hatch the LLM providers have, for an
    /// internal instance behind a corporate CA. `None` falls back to the
    /// build's certificate, if it ships one.
    pub trusted_cert_pem: Option<String>,
}

impl JiraSettings {
    /// Whether enough is configured to even attempt a request. A missing
    /// token is not checked here — settings don't hold it.
    pub fn is_addressable(&self) -> bool {
        !self.base_url.trim().is_empty()
    }
}

/// What the settings tab reads: the user's own values (so the form edits
/// what they set, not what the build supplies) plus what the build would
/// fall back to, so the UI can say so instead of showing an empty field
/// that nonetheless works.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraSettingsView {
    pub settings: JiraSettings,
    pub bundled_base_url: Option<String>,
    /// Deliberately a flag, not the PEM itself — a build certificate is not
    /// secret, but there is nothing the UI would do with several kilobytes
    /// of base64 except make the form unreadable.
    pub has_bundled_cert: bool,
}

/// The authenticated account, as reported by `GET /rest/api/latest/myself`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraUser {
    pub display_name: String,
    pub email_address: Option<String>,
    /// Cloud calls it `accountId`, Server/DC calls it `name`/`key` — either
    /// way it's the stable handle to show under the display name.
    pub account_id: Option<String>,
    pub active: bool,
}

// No avatar here on purpose. Jira serves avatars outside the REST API, from
// `/secure/useravatar`, and answers an *anonymous* request with the generic
// silhouette instead of a 401 — so the webview, which has neither the token
// nor the corporate trust root, would silently show a placeholder with no
// error to fall back from. Making the real picture appear means fetching it
// backend-side and inlining it as a `data:` URI, which costs a second HTTP
// round trip per check and a base64 blob through IPC. Not worth it for
// decoration; the panel shows the name instead.

#[derive(Debug, Error)]
pub enum JiraError {
    #[error("Jira не настроена: укажите адрес инстанса в настройках")]
    NotConfigured,
    #[error("Не сохранён токен доступа Jira")]
    MissingToken,
    #[error("Некорректный адрес Jira: {0}")]
    InvalidBaseUrl(String),
    #[error("Ошибка TLS: {0}")]
    Tls(String),
    #[error("Jira отклонила токен: он неверен, истёк или у него нет прав")]
    Unauthorized,
    #[error("Запрос к Jira не удался: {0}")]
    Request(String),
    #[error("Не удалось прочитать настройки Jira: {0}")]
    Settings(String),
}
