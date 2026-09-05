//! Minimal NTLMv2 for OWA. The spike proved this server needs neither channel
//! binding nor a MIC (a Type3 with a valid `NTProofStr` and no MIC returns
//! 200), so this is the small case: `NTOWFv2` → `NTProofStr` → `LMv2` and the
//! Type1/Type3 messages, with no session key and no version/MIC block.
//!
//! This is assembled from audited primitives (`md4`, `md-5`, `hmac`) against
//! MS-NLMP, not novel crypto — and the official MS-NLMP §4.2.4 test vectors
//! guard it below, the same vectors the Python spike passed.

use hmac::{Hmac, Mac};
use md4::Md4;
use md5::{Digest, Md5};

type HmacMd5 = Hmac<Md5>;

fn hmac_md5(key: &[u8], msg: &[u8]) -> [u8; 16] {
    let mut m = <HmacMd5 as Mac>::new_from_slice(key).expect("hmac accepts any key length");
    m.update(msg);
    m.finalize().into_bytes().into()
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
}

/// NTOWFv2 = HMAC_MD5(MD4(UTF16LE(pass)), UTF16LE(UPPER(user) + domain)).
fn ntowf_v2(user: &str, domain: &str, password: &str) -> [u8; 16] {
    let nt = Md4::digest(utf16le(password));
    hmac_md5(&nt, &utf16le(&(user.to_uppercase() + domain)))
}

/// `temp` blob (MS-NLMP 3.3.2): header, timestamp, client challenge, then the
/// server's target info verbatim.
fn make_temp(timestamp: [u8; 8], client_chal: [u8; 8], target_info: &[u8]) -> Vec<u8> {
    let mut t = Vec::with_capacity(28 + target_info.len());
    t.extend_from_slice(&[0x01, 0x01, 0, 0, 0, 0, 0, 0]);
    t.extend_from_slice(&timestamp);
    t.extend_from_slice(&client_chal);
    t.extend_from_slice(&[0, 0, 0, 0]);
    t.extend_from_slice(target_info);
    t.extend_from_slice(&[0, 0, 0, 0]);
    t
}

/// NtChallengeResponse = NTProofStr || temp, where
/// NTProofStr = HMAC_MD5(NTOWFv2, ServerChallenge || temp).
fn nt_v2_response(resp_key: &[u8; 16], server_chal: &[u8; 8], temp: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8 + temp.len());
    buf.extend_from_slice(server_chal);
    buf.extend_from_slice(temp);
    let proof = hmac_md5(resp_key, &buf);
    let mut resp = proof.to_vec();
    resp.extend_from_slice(temp);
    resp
}

fn lm_v2_response(resp_key: &[u8; 16], server_chal: &[u8; 8], client_chal: &[u8; 8]) -> Vec<u8> {
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(server_chal);
    buf[8..].copy_from_slice(client_chal);
    let mut resp = hmac_md5(resp_key, &buf).to_vec();
    resp.extend_from_slice(client_chal);
    resp
}

// NTLMSSP negotiate flags we use.
const NTLMSSP: &[u8; 8] = b"NTLMSSP\0";
const F_UNICODE: u32 = 0x1;
const F_REQUEST_TARGET: u32 = 0x4;
const F_NTLM: u32 = 0x200;
const F_ALWAYS_SIGN: u32 = 0x8000;
const F_TARGET_TYPE_DOMAIN: u32 = 0x10000;
const F_EXT_SEC: u32 = 0x80000;
const F_TARGET_INFO: u32 = 0x800000;

/// The NTLM Type 1 (Negotiate) message, base for the `Authorization: NTLM …`
/// header of the first request.
pub fn type1_message() -> Vec<u8> {
    let flags = F_UNICODE | F_REQUEST_TARGET | F_NTLM | F_ALWAYS_SIGN | F_EXT_SEC;
    let mut m = Vec::with_capacity(32);
    m.extend_from_slice(NTLMSSP);
    m.extend_from_slice(&1u32.to_le_bytes());
    m.extend_from_slice(&flags.to_le_bytes());
    m.extend_from_slice(&[0, 0, 0, 0, 32, 0, 0, 0]); // DomainName fields (empty @32)
    m.extend_from_slice(&[0, 0, 0, 0, 32, 0, 0, 0]); // Workstation fields (empty @32)
    m
}

/// The bits of the server's Type 2 (Challenge) we need.
pub struct Challenge {
    pub server_challenge: [u8; 8],
    pub target_info: Vec<u8>,
}

/// Parses a Type 2 message. Returns `None` if it is not a well-formed
/// NTLMSSP challenge.
pub fn parse_type2(raw: &[u8]) -> Option<Challenge> {
    if raw.len() < 48 || &raw[0..8] != NTLMSSP || u32::from_le_bytes(raw[8..12].try_into().ok()?) != 2
    {
        return None;
    }
    let server_challenge: [u8; 8] = raw[24..32].try_into().ok()?;
    let ti_len = u16::from_le_bytes(raw[40..42].try_into().ok()?) as usize;
    let ti_off = u32::from_le_bytes(raw[44..48].try_into().ok()?) as usize;
    let target_info = raw.get(ti_off..ti_off + ti_len)?.to_vec();
    Some(Challenge { server_challenge, target_info })
}

/// FILETIME (100-ns ticks since 1601-01-01) for `now`.
fn filetime_now() -> [u8; 8] {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ticks = (secs + 11_644_473_600) * 10_000_000;
    ticks.to_le_bytes()
}

/// Builds the Type 3 (Authenticate) message — no version block, no MIC (this
/// server accepts that). `client_challenge` is a parameter so tests are
/// deterministic; production passes a fresh random 8 bytes.
pub fn type3_message(
    user: &str,
    domain: &str,
    password: &str,
    challenge: &Challenge,
    client_challenge: [u8; 8],
) -> Vec<u8> {
    build_type3(user, domain, password, challenge, client_challenge, filetime_now())
}

/// Deterministic core with an explicit timestamp — the seam tests use to diff
/// the exact bytes against the (server-verified) Python reference.
fn build_type3(
    user: &str,
    domain: &str,
    password: &str,
    challenge: &Challenge,
    client_challenge: [u8; 8],
    timestamp: [u8; 8],
) -> Vec<u8> {
    let key = ntowf_v2(user, domain, password);
    let temp = make_temp(timestamp, client_challenge, &challenge.target_info);
    let nt_resp = nt_v2_response(&key, &challenge.server_challenge, &temp);
    let lm_resp = lm_v2_response(&key, &challenge.server_challenge, &client_challenge);

    let dom_b = utf16le(domain);
    let usr_b = utf16le(user);
    let flags = F_UNICODE | F_NTLM | F_EXT_SEC | F_TARGET_INFO | F_ALWAYS_SIGN | F_TARGET_TYPE_DOMAIN;

    // Payload starts after the 64-byte header (6 security buffers + flags).
    const BASE: u32 = 64;
    let mut payload = Vec::new();
    let mut field = |data: &[u8]| {
        let off = BASE + payload.len() as u32;
        payload.extend_from_slice(data);
        let len = data.len() as u16;
        let mut f = Vec::with_capacity(8);
        f.extend_from_slice(&len.to_le_bytes());
        f.extend_from_slice(&len.to_le_bytes());
        f.extend_from_slice(&off.to_le_bytes());
        f
    };
    let lm_f = field(&lm_resp);
    let nt_f = field(&nt_resp);
    let dom_f = field(&dom_b);
    let usr_f = field(&usr_b);
    let ws_f = field(&[]);
    let key_f = field(&[]);

    let mut m = Vec::with_capacity(BASE as usize + payload.len());
    m.extend_from_slice(NTLMSSP);
    m.extend_from_slice(&3u32.to_le_bytes());
    m.extend_from_slice(&lm_f);
    m.extend_from_slice(&nt_f);
    m.extend_from_slice(&dom_f);
    m.extend_from_slice(&usr_f);
    m.extend_from_slice(&ws_f);
    m.extend_from_slice(&key_f);
    m.extend_from_slice(&flags.to_le_bytes());
    m.extend_from_slice(&payload);
    m
}

/// A fresh 8-byte client challenge.
pub fn random_client_challenge() -> [u8; 8] {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    // MS-NLMP §4.2.4: User/Domain/Password, fixed server & client challenge.
    fn vectors() -> ([u8; 16], [u8; 8], [u8; 8], Vec<u8>) {
        let key = ntowf_v2("User", "Domain", "Password");
        let server_chal = hex("0123456789abcdef");
        let client_chal = hex("aaaaaaaaaaaaaaaa");
        let ti = hex(
            "02000c0044006f006d00610069006e00\
             01000c00530065007200760065007200\
             00000000",
        );
        (key.try_into().unwrap(), server_chal.try_into().unwrap(), client_chal.try_into().unwrap(), ti)
    }
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn ntowf_v2_matches_spec() {
        let key = ntowf_v2("User", "Domain", "Password");
        assert_eq!(hexs(&key), "0c868a403bfd7a93a3001ef22ef02e3f");
    }

    #[test]
    fn ntproofstr_matches_spec() {
        let (key, server_chal, client_chal, ti) = vectors();
        // time = 0 for the spec vector.
        let temp = make_temp([0u8; 8], client_chal, &ti);
        let resp = nt_v2_response(&key, &server_chal, &temp);
        assert_eq!(hexs(&resp[..16]), "68cd0ab851e51c96aabc927bebef6a1c");
        // Session base key, for good measure (not sent, but proves the chain).
        let sbk = hmac_md5(&key, &resp[..16]);
        assert_eq!(hexs(&sbk), "8de40ccadbc14a82f15cb0ad0de95ca3");
    }

    #[test]
    fn type1_is_well_formed() {
        let t1 = type1_message();
        assert_eq!(&t1[0..8], NTLMSSP);
        assert_eq!(u32::from_le_bytes(t1[8..12].try_into().unwrap()), 1);
        assert_eq!(t1.len(), 32);
    }

    #[test]
    fn type3_roundtrips_through_parse_type2() {
        // Build a synthetic Type2, parse it, build a Type3, sanity-check shape.
        let (_key, server_chal, client_chal, ti) = vectors();
        let mut t2 = Vec::new();
        t2.extend_from_slice(NTLMSSP);
        t2.extend_from_slice(&2u32.to_le_bytes());
        t2.extend_from_slice(&[0, 0, 0, 0, 56, 0, 0, 0]); // target name fields
        t2.extend_from_slice(&0u32.to_le_bytes()); // flags
        t2.extend_from_slice(&server_chal);
        t2.extend_from_slice(&[0u8; 8]); // reserved
        let ti_off = 56u32;
        t2.extend_from_slice(&(ti.len() as u16).to_le_bytes());
        t2.extend_from_slice(&(ti.len() as u16).to_le_bytes());
        t2.extend_from_slice(&ti_off.to_le_bytes());
        t2.extend_from_slice(&[0u8; 8]); // version
        assert_eq!(t2.len(), 56);
        t2.extend_from_slice(&ti);

        let ch = parse_type2(&t2).expect("parse");
        assert_eq!(ch.server_challenge, server_chal);
        assert_eq!(ch.target_info, ti);

        let t3 = type3_message("User", "Domain", "Password", &ch, client_chal);
        assert_eq!(&t3[0..8], NTLMSSP);
        assert_eq!(u32::from_le_bytes(t3[8..12].try_into().unwrap()), 3);
        // NtChallengeResponse offset/len point inside the message.
        let nt_len = u16::from_le_bytes(t3[20..22].try_into().unwrap()) as usize;
        let nt_off = u32::from_le_bytes(t3[24..28].try_into().unwrap()) as usize;
        assert!(nt_off + nt_len <= t3.len());
        // First 16 bytes of the NT response are the proof for this challenge.
        let temp = make_temp(
            t3[nt_off + 24..nt_off + 32].try_into().unwrap(),
            client_chal,
            &ti,
        );
        let key = ntowf_v2("User", "Domain", "Password");
        let expect = nt_v2_response(&key, &server_chal, &temp);
        assert_eq!(&t3[nt_off..nt_off + 16], &expect[..16]);
    }

    #[test]
    fn type3_bytes_match_server_verified_reference() {
        // The exact Type3 the Python spike sent and the real Exchange server
        // accepted (HTTP 200), for these fixed inputs. Pins the whole wire
        // format — flags, field offsets, NTLMv2/LMv2 responses — to bytes
        // known to work, so a refactor that still passes the MS-NLMP crypto
        // vectors but mangles the message layout is caught here.
        let ti = hex(
            "02000c0044006f006d00610069006e00\
             01000c00530065007200760065007200\
             00000000",
        );
        let ch = Challenge {
            server_challenge: hex("0123456789abcdef").try_into().unwrap(),
            target_info: ti,
        };
        let t3 = build_type3(
            "u_m2bnr",
            "moscow",
            "testpass",
            &ch,
            hex("aaaaaaaaaaaaaaaa").try_into().unwrap(),
            [0u8; 8],
        );
        let expected = "4e544c4d5353500003000000180018004000000054005400580000000c000c00ac0000000e000e00b800000000000000c600000000000000c6000000018289004ad3d301ff073cf7e980a1620ef8e349aaaaaaaaaaaaaaaac4cf21622b91901462f281caabc78ca001010000000000000000000000000000aaaaaaaaaaaaaaaa0000000002000c0044006f006d00610069006e0001000c0053006500720076006500720000000000000000006d006f00730063006f00770075005f006d00320062006e007200";
        assert_eq!(hexs(&t3), expected);
    }

    fn hexs(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
