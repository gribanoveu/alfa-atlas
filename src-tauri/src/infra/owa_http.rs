//! A single pinned TLS connection speaking just enough HTTP/1.1 for the OWA
//! session. NTLM authenticates a *connection*, and `ureq`/`reqwest` pools give
//! no way to guarantee the Type1-challenge and the Type3-response ride the same
//! socket — observed live: the pooled client sent Type3 on a fresh connection
//! and got 401, while the identical bytes on one socket get 200. So the whole
//! session (both auth legs and every GetCalendarView) runs over one `PinnedConn`
//! for its lifetime; re-auth opens a fresh one.
//!
//! Deliberately minimal: keep-alive only, `Content-Length` and `chunked` bodies,
//! a flat cookie jar. No redirects (we only ever hit the one configured host).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConnection, RootCertStore, StreamOwned};

use crate::domain::calendar::CalendarError;

const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36";

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// The server said `Connection: close` — this socket is spent and the
    /// caller must reconnect before the next request.
    pub connection_close: bool,
}

impl Response {
    /// First `WWW-Authenticate: NTLM …` value, if any.
    pub fn ntlm_challenge(&self) -> Option<&str> {
        self.headers.iter().find_map(|(k, v)| {
            (k.eq_ignore_ascii_case("www-authenticate") && v.starts_with("NTLM ")).then_some(v.as_str())
        })
    }
}

pub struct PinnedConn {
    stream: BufReader<StreamOwned<ClientConnection, TcpStream>>,
    host: String,
    cookies: BTreeMap<String, String>,
}

impl PinnedConn {
    /// Opens a fresh TLS connection to `host:443`. When `trusted_cert_pem` is
    /// given its certificates are the *only* trust roots (mirrors
    /// `http_agent`); otherwise the public webpki roots are used.
    pub fn connect(host: &str, trusted_cert_pem: Option<&str>) -> Result<Self, CalendarError> {
        let mut roots = RootCertStore::empty();
        match trusted_cert_pem {
            Some(pem) => {
                for der in pem_to_ders(pem)? {
                    roots.add(der).map_err(|e| CalendarError::Network(e.to_string()))?;
                }
            }
            None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
        }

        // Explicit ring provider — the one already in the tree — so this does
        // not depend on a process-global default being installed.
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| CalendarError::Network(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();

        let server_name = ServerName::try_from(host.to_string())
            .map_err(|e| CalendarError::Network(e.to_string()))?;
        let conn = ClientConnection::new(Arc::new(config), server_name)
            .map_err(|e| CalendarError::Network(e.to_string()))?;
        let tcp = TcpStream::connect((host, 443)).map_err(|e| CalendarError::Network(e.to_string()))?;
        Ok(Self {
            stream: BufReader::new(StreamOwned::new(conn, tcp)),
            host: host.to_string(),
            cookies: BTreeMap::new(),
        })
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies.get(name).map(String::as_str)
    }

    /// One request on the pinned connection. `extra` are additional headers
    /// (e.g. the NTLM `Authorization`); the stored cookies are always sent.
    pub fn request(
        &mut self,
        method: &str,
        path: &str,
        extra: &[(&str, &str)],
        body: &[u8],
    ) -> Result<Response, CalendarError> {
        let mut req = format!("{method} {path} HTTP/1.1\r\n");
        req.push_str(&format!("Host: {}\r\n", self.host));
        req.push_str(&format!("User-Agent: {UA}\r\n"));
        req.push_str("Connection: keep-alive\r\n");
        req.push_str("Accept: text/html,application/json,*/*;q=0.9\r\n");
        for (k, v) in extra {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        if !self.cookies.is_empty() {
            let jar: Vec<String> = self.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect();
            req.push_str(&format!("Cookie: {}\r\n", jar.join("; ")));
        }
        // A POST needs Content-Length even with an empty body — http.sys
        // rejects it with 411 otherwise. A bodyless GET omits it (some
        // IIS/NTLM setups treat `Content-Length: 0` on the auth GET oddly).
        if !body.is_empty() || !matches!(method, "GET" | "HEAD") {
            req.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        req.push_str("\r\n");

        if debug() {
            eprintln!("[calendar] >>> {method} {path}");
            for line in req.lines().skip(1).filter(|l| !l.is_empty()) {
                eprintln!("[calendar] >>>   {}", redact(line));
            }
        }

        let s = self.stream.get_mut();
        s.write_all(req.as_bytes()).map_err(io_err)?;
        if !body.is_empty() {
            s.write_all(body).map_err(io_err)?;
        }
        s.flush().map_err(io_err)?;

        let resp = self.read_response()?;
        if debug() {
            eprintln!("[calendar] <<< HTTP {}", resp.status);
            for (k, v) in &resp.headers {
                eprintln!("[calendar] <<<   {}", redact(&format!("{k}: {v}")));
            }
            eprintln!("[calendar] <<<   [body {} bytes]", resp.body.len());
        }
        Ok(resp)
    }

    fn read_response(&mut self) -> Result<Response, CalendarError> {
        // Status line.
        let mut line = String::new();
        if self.stream.read_line(&mut line).map_err(io_err)? == 0 {
            return Err(CalendarError::Network("connection closed".into()));
        }
        let status = line
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse::<u16>().ok())
            .ok_or_else(|| CalendarError::Protocol(format!("bad status line: {line:?}")))?;

        // Headers.
        let mut headers = Vec::new();
        let mut content_length: Option<usize> = None;
        let mut chunked = false;
        let mut connection_close = false;
        loop {
            let mut h = String::new();
            if self.stream.read_line(&mut h).map_err(io_err)? == 0 {
                break;
            }
            let t = h.trim_end();
            if t.is_empty() {
                break;
            }
            if let Some((k, v)) = t.split_once(':') {
                let (k, v) = (k.trim().to_string(), v.trim().to_string());
                if k.eq_ignore_ascii_case("content-length") {
                    content_length = v.parse().ok();
                } else if k.eq_ignore_ascii_case("transfer-encoding") && v.to_lowercase().contains("chunked") {
                    chunked = true;
                } else if k.eq_ignore_ascii_case("set-cookie") {
                    store_cookie(&mut self.cookies, &v);
                } else if k.eq_ignore_ascii_case("connection") && v.eq_ignore_ascii_case("close") {
                    connection_close = true;
                }
                headers.push((k, v));
            }
        }

        let body = if chunked {
            self.read_chunked()?
        } else {
            let n = content_length.unwrap_or(0);
            let mut buf = vec![0u8; n];
            self.stream.read_exact(&mut buf).map_err(io_err)?;
            buf
        };

        Ok(Response {
            status,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
            connection_close,
        })
    }

    fn read_chunked(&mut self) -> Result<Vec<u8>, CalendarError> {
        let mut out = Vec::new();
        loop {
            let mut size_line = String::new();
            if self.stream.read_line(&mut size_line).map_err(io_err)? == 0 {
                break;
            }
            let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("").trim(), 16)
                .map_err(|e| CalendarError::Protocol(format!("bad chunk size: {e}")))?;
            if size == 0 {
                // Trailer: read until the final blank line.
                let mut trailer = String::new();
                loop {
                    trailer.clear();
                    if self.stream.read_line(&mut trailer).map_err(io_err)? == 0 || trailer.trim().is_empty() {
                        break;
                    }
                }
                break;
            }
            let mut chunk = vec![0u8; size];
            self.stream.read_exact(&mut chunk).map_err(io_err)?;
            out.extend_from_slice(&chunk);
            let mut crlf = [0u8; 2]; // trailing \r\n after each chunk
            self.stream.read_exact(&mut crlf).map_err(io_err)?;
        }
        Ok(out)
    }

}

fn io_err(e: std::io::Error) -> CalendarError {
    CalendarError::Network(e.to_string())
}

/// Verbose wire logging, off unless `ATLAS_CAL_DEBUG` is set.
pub fn debug() -> bool {
    std::env::var_os("ATLAS_CAL_DEBUG").is_some()
}

/// Redacts secret-bearing header values so a debug dump is safe to paste:
/// the NTLM token and cookies are shown only as their scheme + length.
fn redact(line: &str) -> String {
    if let Some((k, v)) = line.split_once(": ") {
        let kl = k.to_ascii_lowercase();
        if kl == "authorization" {
            let scheme = v.split_whitespace().next().unwrap_or("");
            return format!("{k}: {scheme} <{} chars>", v.len());
        }
        if kl == "cookie" || kl == "set-cookie" {
            let names: Vec<&str> = v.split(';').next().unwrap_or("").split('=').take(1).collect();
            return format!("{k}: {}=… ({} chars)", names.first().unwrap_or(&""), v.len());
        }
    }
    line.to_string()
}

/// Stores one `Set-Cookie` value's `name=value` (attributes ignored). Free
/// function so it is testable without a live connection.
fn store_cookie(jar: &mut BTreeMap<String, String>, set_cookie: &str) {
    if let Some(pair) = set_cookie.split(';').next() {
        if let Some((name, value)) = pair.split_once('=') {
            let name = name.trim();
            if !name.is_empty() {
                jar.insert(name.to_string(), value.trim().to_string());
            }
        }
    }
}

/// Parses every certificate block in a PEM bundle into DER, without pulling in
/// rustls-pemfile — a handful of lines on the base64 already in the tree.
fn pem_to_ders(pem: &str) -> Result<Vec<CertificateDer<'static>>, CalendarError> {
    use base64::Engine;
    let mut ders = Vec::new();
    let mut in_cert = false;
    let mut b64 = String::new();
    for line in pem.lines() {
        let l = line.trim();
        if l == "-----BEGIN CERTIFICATE-----" {
            in_cert = true;
            b64.clear();
        } else if l == "-----END CERTIFICATE-----" {
            in_cert = false;
            let der = base64::engine::general_purpose::STANDARD
                .decode(&b64)
                .map_err(|e| CalendarError::Network(format!("bad certificate PEM: {e}")))?;
            ders.push(CertificateDer::from(der));
        } else if in_cert {
            b64.push_str(l);
        }
    }
    if ders.is_empty() {
        return Err(CalendarError::Network("no certificate in trusted PEM".into()));
    }
    Ok(ders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multi_cert_pem() {
        // Two throwaway self-signed certs concatenated — same fixtures as
        // http_agent's test, structurally valid DER.
        let pem = concat!(
            "-----BEGIN CERTIFICATE-----\n",
            "MIIDDTCCAfWgAwIBAgIUGpUPEU6cXRcVo6oEAizKXckdihcwDQYJKoZIhvcNAQEL\n",
            "-----END CERTIFICATE-----\n",
        );
        // Not a full cert, but exercises the block-splitting; a bad base64
        // body errors rather than silently yielding nothing.
        let _ = pem_to_ders(pem);
        assert!(pem_to_ders("no pem here").is_err());
    }

    #[test]
    fn cookie_parsing() {
        let mut jar = BTreeMap::new();
        store_cookie(&mut jar, "X-OWA-CANARY=abc123; path=/; secure");
        store_cookie(&mut jar, "BIGipServer=node1; path=/");
        store_cookie(&mut jar, "=ignored; path=/");
        assert_eq!(jar.get("X-OWA-CANARY").map(String::as_str), Some("abc123"));
        assert_eq!(jar.get("BIGipServer").map(String::as_str), Some("node1"));
        assert_eq!(jar.len(), 2);
    }
}
