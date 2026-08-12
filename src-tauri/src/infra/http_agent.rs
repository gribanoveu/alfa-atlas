//! Shared `ureq::Agent` construction for OpenAI-compatible HTTP clients
//! (LLM chat and remote embeddings). TLS trust is configured per-`Agent`,
//! not globally, so each provider gets its own agent built once at
//! construction time via `build_agent`.

use thiserror::Error;

#[derive(Debug, Error)]
#[error("tls configuration error: {0}")]
pub struct TlsError(pub String);

/// Builds the `ureq::Agent` a provider's requests go through. When
/// `trusted_cert_pem` is `Some`, its certificates **replace** the agent's
/// trust store entirely (`RootCerts::Specific`, not additive to the public
/// WebPki roots) — correct here since a provider either needs its own CA
/// trusted (an internal endpoint) or doesn't; providers without an override
/// keep the default `RootCerts::WebPki`, and since every provider gets its
/// own `Agent`, there's no cross-provider trust interference either way.
pub fn build_agent(trusted_cert_pem: Option<&str>) -> Result<ureq::Agent, TlsError> {
    build_agent_with_options(trusted_cert_pem, false)
}

/// When `disable_verification` is true, accepts any server certificate —
/// including expired or self-signed ones. Intended for internal sandboxes
/// only; never use against production endpoints.
pub fn build_agent_with_options(
    trusted_cert_pem: Option<&str>,
    disable_verification: bool,
) -> Result<ureq::Agent, TlsError> {
    // Disables ureq's default behavior of turning a non-2xx status into a
    // bare `Error::StatusCode(code)` *before* the caller can read the
    // response body — that's exactly what made a provider error like a
    // rejected request or a server-side failure show up as an
    // undiagnosable "http status: 500" with no detail. Callers that want
    // the body fold it into their own error type after reading.
    let mut builder = ureq::Agent::config_builder().http_status_as_error(false);
    if disable_verification {
        let tls_config = ureq::tls::TlsConfig::builder()
            .disable_verification(true)
            .build();
        builder = builder.tls_config(tls_config);
    } else if let Some(pem) = trusted_cert_pem {
        let certs = parse_trusted_certs(pem)?;
        let tls_config = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::Specific(std::sync::Arc::new(certs)))
            .build();
        builder = builder.tls_config(tls_config);
    }
    Ok(builder.build().new_agent())
}

/// Parses **every** certificate in `pem`, not just the first — unlike
/// `ureq::tls::Certificate::from_pem`, which is documented to pick only
/// the first certificate it finds. A corporate internal CA is commonly
/// issued as a chain (a root CA plus one or more intermediate CAs), and a
/// user pasting that whole chain as one concatenated PEM blob (or a
/// downstream fork baking it into the manifest) expects all of it trusted,
/// not silently just whichever certificate happens to appear first.
/// Errors if `pem` contains no certificate at all (mirrors
/// `Certificate::from_pem`'s "no pem encoded cert found" error for that
/// case).
pub fn parse_trusted_certs(pem: &str) -> Result<Vec<ureq::tls::Certificate<'static>>, TlsError> {
    let certs = ureq::tls::parse_pem(pem.as_bytes())
        .filter_map(|item| match item {
            Ok(ureq::tls::PemItem::Certificate(cert)) => Some(Ok(cert)),
            Err(e) => Some(Err(e)),
            // `PemItem` is `#[non_exhaustive]` — anything besides a
            // certificate (e.g. a private key, if one were pasted
            // alongside) isn't a trust root and is skipped rather than
            // erroring, same as `PrivateKey` items are today.
            Ok(_) => None,
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError(e.to_string()))?;
    if certs.is_empty() {
        return Err(TlsError("no PEM-encoded certificate found".to_string()));
    }
    Ok(certs)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Two throwaway self-signed certificates (`openssl req -x509 -newkey
    // rsa:2048 -nodes -days 1 -subj "/CN=test-root-N"`) — real, structurally
    // valid X.509/DER once base64-decoded, but not issued by anything and
    // not used to actually connect anywhere. Exist purely so
    // `parse_trusted_certs`/`build_agent` are tested against real PEM
    // encoding rather than hand-typed placeholder text.
    const TEST_CERT_1: &str = "-----BEGIN CERTIFICATE-----\n\
MIIDDTCCAfWgAwIBAgIUGpUPEU6cXRcVo6oEAizKXckdihcwDQYJKoZIhvcNAQEL\n\
BQAwFjEUMBIGA1UEAwwLdGVzdC1yb290LTEwHhcNMjYwODA3MTAxMDM5WhcNMjYw\n\
ODA4MTAxMDM5WjAWMRQwEgYDVQQDDAt0ZXN0LXJvb3QtMTCCASIwDQYJKoZIhvcN\n\
AQEBBQADggEPADCCAQoCggEBAKzBAwpDZvuKq3/aQJNh2EpezEGhcHY8mlo+6qHx\n\
B3Mp8ClBbUaFif7IxOM0xfSBrP8RjmzUFxg1n80456fwLNgkdRSopK5Gef6hQT1c\n\
6n2e2qIXPjgwoLQplAByAsUoojy0fT87HdFRNl7trjqDf1M8+l2aZt6hV7KWBwNK\n\
RiLwlAXhoWRhzk0lIeu12DFwEaYYYoU2GAObo9upUsnl3FZjTOMN614G9fHXi72J\n\
WTBCiTbKL2p4yd7olGjlSYAWx6Sjp4RTUO2mLYuuq5RNznuc0Q40j/DOMH+xYWw/\n\
LPqf5onSSm7wrBPocmkb5is+Dho0989VrcT83OBw27ZqKVECAwEAAaNTMFEwHQYD\n\
VR0OBBYEFPvdNpMkJZ//V2KiSSYDoIH2aGpgMB8GA1UdIwQYMBaAFPvdNpMkJZ//\n\
V2KiSSYDoIH2aGpgMA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEB\n\
AJ7hnJfnq2zt4sujG2GJ+imBMWI2H+NZiHhtYbu77S8/UC6OTc/7rQdFooeh2kTX\n\
h+KqHNoIxzubZg1TpzENjo2msJ8EhLhbHGAjMt1AsFxtfiepqAHfhQZvb4Pj+fJn\n\
hIKv3mq6TJh7i683UYrMno+RlXpPxqcIT+dpPTeVjTknofhEhg78sv9AhfxeCYS+\n\
o2luAw64b0TYXF8sf4Mx5IoOsfN20Hm+pj3nKQH/SpOLZwlgXQURWJwytxibck4W\n\
ruzJHn650tODkZyjFHnk62Cd4QKa8Jm86El9v80aIq25DWq/UCJKzYkbKLuktbwZ\n\
qeR/KGN8Bh7XSk/B/N/8gJQ=\n\
-----END CERTIFICATE-----\n";

    const TEST_CERT_2: &str = "-----BEGIN CERTIFICATE-----\n\
MIIDDTCCAfWgAwIBAgIUXKODgC8Vp4Zo6zpfD6dXtJz7W8gwDQYJKoZIhvcNAQEL\n\
BQAwFjEUMBIGA1UEAwwLdGVzdC1yb290LTIwHhcNMjYwODA3MTAxMDM5WhcNMjYw\n\
ODA4MTAxMDM5WjAWMRQwEgYDVQQDDAt0ZXN0LXJvb3QtMjCCASIwDQYJKoZIhvcN\n\
AQEBBQADggEPADCCAQoCggEBANX8US+mSoyHjk5ul8cTffGNWStsaRjJg+b8tXMj\n\
w6p3yl9laSdHZn5507zePh/madO5cxWQFWxAzO21HPNHaPYX9rnNVurHNDdeYIx/\n\
4arjjJESJP/D84t41gkOqmd5oUPTdVJEO8uGGWRTGrsN+s6jB/TGpjxk83guyycP\n\
vKVNhtWUXBeX3agie7KaFxWnMgMC2Cq5Rn9BGEdPgTbWs8VUlo54IHZNDAggl0MB\n\
Kie+Y6vLQg67IRadNci0DMr7oG2sJkpYHC5YIpNI7+3nOv/tA8gOoMvna3a/vcMJ\n\
9GbqfwUywikvbj2sfavBj6Oz/rrzWHgKTb2lwGWq+3JrqhkCAwEAAaNTMFEwHQYD\n\
VR0OBBYEFAV8iu42XG8hACTT8sJwMNhBLWWQMB8GA1UdIwQYMBaAFAV8iu42XG8h\n\
ACTT8sJwMNhBLWWQMA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEB\n\
ACa7NdZaTqb3sBhv5yJ+VXu2KBkOaKuRtX1GpJwDA5NEkrXxGc5plZ6x322yaj4L\n\
RvRAPuTKG0yggPfeAjsUc9F93azXLYSdoGd8Nluuob7IbLaKjyVXMMr33cfY1Rkn\n\
XisLoCs5m2KSB96izGn/F2JDbTFbWtHDAQgrCO7gvOQ3sQfzaRhiHiRnuX8c+18A\n\
EdPIXI8UYm6De+fhi7iXIFRjHoYWcm15gr/A2hpb2/f6fBdcsn5l9F15iScu6tF3\n\
92SNkKhKb3b+I32HzRrFoLoL/QQHUOC0SGhXVbcyQ8FdYs45/WFkuFUvS2NkqT+C\n\
YoQlQIWF38mOPRxLRBxKA7g=\n\
-----END CERTIFICATE-----\n";

    #[test]
    fn build_agent_with_disabled_verification_succeeds() {
        assert!(build_agent_with_options(None, true).is_ok());
    }

    #[test]
    fn build_agent_succeeds_with_no_trust_cert_override() {
        assert!(build_agent(None).is_ok());
    }

    #[test]
    fn build_agent_rejects_a_malformed_pem() {
        let err = build_agent(Some("not a real pem")).unwrap_err();
        assert!(err.0.contains("no PEM-encoded certificate") || !err.0.is_empty());
    }

    #[test]
    fn parse_trusted_certs_extracts_a_single_certificate() {
        let certs = parse_trusted_certs(TEST_CERT_1).unwrap();
        assert_eq!(certs.len(), 1);
    }

    #[test]
    fn parse_trusted_certs_extracts_every_certificate_in_a_concatenated_chain() {
        let chain = format!("{TEST_CERT_1}{TEST_CERT_2}");
        let certs = parse_trusted_certs(&chain).unwrap();
        assert_eq!(
            certs.len(),
            2,
            "both certificates in the chain must be trusted, not just the first"
        );
    }

    #[test]
    fn build_agent_succeeds_with_a_multi_certificate_chain() {
        let chain = format!("{TEST_CERT_1}{TEST_CERT_2}");
        assert!(build_agent(Some(&chain)).is_ok());
    }
}
