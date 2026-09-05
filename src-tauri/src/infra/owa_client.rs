//! OWA JSON API wire format: the `GetCalendarView` request payload and the
//! parsing of its response into `domain::calendar::CalendarEvent`. Pure —
//! no HTTP and no auth here (those are `owa_agent` + `owa_auth`), so this half
//! is fully unit-tested against the reference sample payloads offline.
//!
//! We always request `TimeZoneContext.Id = "UTC"` and
//! `DistinguishedFolderId: "calendar"` — the spike confirmed both work on the
//! target server on the first call after login (no `GetCalendarFolders` round
//! trip, no "abstract class" 500 to work around).

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::domain::calendar::{
    detect_meeting_url, parse_owa_date, safe_url, CalendarEvent, EventAttendee, EventDetails,
    MeetingPlatform, MeetingResponseType, RsvpAction,
};

/// Builds the JSON that goes (URL-encoded) into the `X-OWA-UrlPostData`
/// header. Range bounds are naive `%Y-%m-%dT%H:%M:%S` — with `TimeZoneContext`
/// = UTC that naive time *is* UTC, so the caller passes UTC instants (the
/// frontend computes them from the viewer's day boundaries).
pub fn calendar_view_payload(range_start: DateTime<Utc>, range_end: DateTime<Utc>) -> String {
    // Built as a fixed-order string, NOT via `serde_json::json!` — Exchange's
    // deserializer requires the `__type` discriminator to be the *first* key of
    // every object, but `serde_json::Value` sorts keys alphabetically (no
    // `preserve_order` feature), which pushes `__type` last and makes the
    // server throw `System.MemberAccessException` ("cannot create an abstract
    // class"). The only variable parts are the two formatted UTC instants.
    let fmt = |d: DateTime<Utc>| d.format("%Y-%m-%dT%H:%M:%S").to_string();
    format!(
        concat!(
            r#"{{"__type":"GetCalendarViewJsonRequest:#Exchange","#,
            r#""Header":{{"__type":"JsonRequestHeaders:#Exchange","#,
            r#""RequestServerVersion":"V2017_08_18","#,
            r#""TimeZoneContext":{{"__type":"TimeZoneContext:#Exchange","#,
            r#""TimeZoneDefinition":{{"__type":"TimeZoneDefinitionType:#Exchange","Id":"UTC"}}}}}},"#,
            r#""Body":{{"__type":"GetCalendarViewRequest:#Exchange","#,
            r#""CalendarId":{{"__type":"TargetFolderId:#Exchange","#,
            r#""BaseFolderId":{{"__type":"DistinguishedFolderId:#Exchange","Id":"calendar"}}}},"#,
            r#""RangeStart":"{start}","RangeEnd":"{end}"}}}}"#,
        ),
        start = fmt(range_start),
        end = fmt(range_end),
    )
}

#[derive(Debug, Deserialize)]
struct ServiceResponse {
    #[serde(rename = "Body")]
    body: Option<ResponseBody>,
}

#[derive(Debug, Deserialize)]
struct ResponseBody {
    #[serde(rename = "Items")]
    items: Option<Vec<OwaCalendarItem>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OwaCalendarItem {
    item_id: Option<OwaItemId>,
    subject: Option<String>,
    start: Option<String>,
    end: Option<String>,
    is_all_day_event: Option<bool>,
    is_cancelled: Option<bool>,
    is_organizer: Option<bool>,
    response_type: Option<String>,
    location: Option<OwaLocation>,
    organizer: Option<OwaOrganizer>,
    preview: Option<String>,
    join_online_meeting_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OwaItemId {
    id: String,
    change_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OwaLocation {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OwaOrganizer {
    mailbox: Option<OwaMailbox>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OwaMailbox {
    name: Option<String>,
}

/// Builds the `GetCalendarEvent` request body (sent as the HTTP body, unlike
/// `GetCalendarView`). `__type` first in every object, same reason as the view
/// payload. `id`/`change_key` are base64 from the server — encoded via
/// `serde_json` so any special character is escaped rather than trusted.
pub fn calendar_event_payload(id: &str, change_key: Option<&str>) -> String {
    let id_j = serde_json::to_string(id).unwrap_or_else(|_| "\"\"".to_string());
    let ck = change_key
        .map(|k| format!(r#","ChangeKey":{}"#, serde_json::to_string(k).unwrap_or_default()))
        .unwrap_or_default();
    format!(
        concat!(
            r#"{{"__type":"GetCalendarEventJsonRequest:#Exchange","#,
            r#""Header":{{"__type":"JsonRequestHeaders:#Exchange","RequestServerVersion":"V2017_08_18","#,
            r#""TimeZoneContext":{{"__type":"TimeZoneContext:#Exchange","#,
            r#""TimeZoneDefinition":{{"__type":"TimeZoneDefinitionType:#Exchange","Id":"UTC"}}}}}},"#,
            r#""Body":{{"__type":"GetCalendarEventRequest:#Exchange","#,
            r#""EventIds":[{{"__type":"ItemId:#Exchange","Id":{id}{ck}}}],"#,
            r#""ItemShape":{{"__type":"ItemResponseShape:#Exchange","BaseShape":"IdOnly","#,
            r#""FilterHtmlContent":true,"AddBlankTargetToLinks":true,"BodyType":"HTML"}}}}}}"#,
        ),
        id = id_j,
        ck = ck,
    )
}

/// Builds the EWS `CreateItem` SOAP for an RSVP. `send` chooses
/// `SendAndSaveCopy` (notify the organizer) vs `SaveOnly` (update only the
/// user's own calendar — a silent RSVP). `item_id`/`change_key` go into XML
/// *attributes*, so they are attribute-escaped (`"` included) — the reference
/// uses text escaping here, which would break on a base64 id containing `"`.
pub fn rsvp_soap(action: RsvpAction, item_id: &str, change_key: &str, send: bool) -> String {
    let disposition = if send { "SendAndSaveCopy" } else { "SaveOnly" };
    let element = action.ews_element();
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/" "#,
            r#"xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types" "#,
            r#"xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages">"#,
            r#"<soap:Header><t:RequestServerVersion Version="Exchange2013_SP1"/></soap:Header>"#,
            r#"<soap:Body><m:CreateItem MessageDisposition="{disposition}"><m:Items>"#,
            r#"<t:{element}><t:ReferenceItemId Id="{id}" ChangeKey="{ck}"/></t:{element}>"#,
            r#"</m:Items></m:CreateItem></soap:Body></soap:Envelope>"#,
        ),
        disposition = disposition,
        element = element,
        id = escape_attr(item_id),
        ck = escape_attr(change_key),
    )
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// EWS returns HTTP 200 even for logical failures, so success is decided by the
/// body: `ResponseClass="Success"` / `<m:ResponseCode>NoError</m:ResponseCode>`.
/// Returns the error code string on failure.
pub fn ews_response_ok(body: &str) -> Result<(), String> {
    if body.contains("<m:ResponseCode>NoError</m:ResponseCode>")
        || body.contains(r#"ResponseClass="Success""#)
    {
        return Ok(());
    }
    // Pull out the ResponseCode for a useful message, if present.
    let code = body
        .split_once("<m:ResponseCode>")
        .and_then(|(_, rest)| rest.split_once("</m:ResponseCode>"))
        .map(|(code, _)| code.to_string())
        .unwrap_or_else(|| "unknown EWS error".to_string());
    Err(code)
}

/// Parses a `GetCalendarEvent` response into attendees + a text body preview.
/// The item's exact nesting varies across Exchange versions, so rather than
/// assume a fixed path this walks the whole tree for the `RequiredAttendees` /
/// `OptionalAttendees` keys and the body fields (as the OWA spec recommends),
/// and uses `Value` for the attendee shapes (bare array, single object, or
/// `{ "Attendee": [...] }`).
pub fn parse_event_details(raw: &str) -> Result<EventDetails, serde_json::Error> {
    use serde_json::Value;
    let v: Value = serde_json::from_str(raw)?;

    let mut attendees = Vec::new();
    // Body candidates keyed by field, in priority order: plain text wins,
    // then the HTML/normalized variants (stripped to text).
    let mut bodies: std::collections::HashMap<&'static str, String> = Default::default();
    walk_event(&v, &mut attendees, &mut bodies);

    // Deduplicate — a recursive walk can meet the same list twice.
    attendees.dedup_by(|a, b| a.name == b.name && a.email == b.email && a.required == b.required);

    let body_text = ["TextBody", "UniqueBody", "Body", "NormalizedBody"]
        .into_iter()
        .find_map(|k| bodies.get(k).cloned())
        .filter(|s| !s.is_empty());

    Ok(EventDetails { attendees, body_text })
}

fn walk_event(
    v: &serde_json::Value,
    attendees: &mut Vec<EventAttendee>,
    bodies: &mut std::collections::HashMap<&'static str, String>,
) {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            if let Some(x) = map.get("RequiredAttendees") {
                collect_attendees(x, true, attendees);
            }
            if let Some(x) = map.get("OptionalAttendees") {
                collect_attendees(x, false, attendees);
            }
            for key in ["TextBody", "UniqueBody", "Body", "NormalizedBody"] {
                if bodies.contains_key(key) {
                    continue;
                }
                if let Some(raw) = map.get(key).and_then(|b| b.get("Value")).and_then(|v| v.as_str())
                {
                    let text = if key == "TextBody" { raw.trim().to_string() } else { strip_html(raw) };
                    if !text.is_empty() {
                        bodies.insert(key, text);
                    }
                }
            }
            for child in map.values() {
                walk_event(child, attendees, bodies);
            }
        }
        Value::Array(a) => {
            for child in a {
                walk_event(child, attendees, bodies);
            }
        }
        _ => {}
    }
}

fn collect_attendees(list: &serde_json::Value, required: bool, out: &mut Vec<EventAttendee>) {
    use serde_json::Value;
    let items: Vec<&Value> = match list {
        Value::Array(a) => a.iter().collect(),
        Value::Object(o) => match o.get("Attendee") {
            Some(Value::Array(a)) => a.iter().collect(),
            Some(one) => vec![one],
            None => vec![list], // a single attendee object
        },
        _ => return,
    };
    for a in items {
        let name = a["Mailbox"]["Name"]
            .as_str()
            .or_else(|| a["Name"].as_str())
            .map(str::to_string);
        let Some(name) = name.filter(|n| !n.trim().is_empty()) else { continue };
        let email = a["Mailbox"]["EmailAddress"]
            .as_str()
            .or_else(|| a["EmailAddress"].as_str())
            .map(str::to_string);
        out.push(EventAttendee {
            name,
            email,
            required,
            response: MeetingResponseType::from_owa(a["ResponseType"].as_str()),
        });
    }
}

/// Reduces meeting-body HTML to a plain-text preview: drop tags, decode the
/// few common entities, collapse whitespace. The panel shows text only — it
/// never renders server HTML.
fn strip_html(html: &str) -> String {
    static TAG: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?s)<[^>]+>").expect("static regex"));
    let no_tags = TAG.replace_all(html, " ");
    let decoded = no_tags
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parses a raw `GetCalendarView` JSON body into events. Items missing an id
/// or unparseable dates are skipped rather than failing the whole sync.
pub fn parse_calendar_view(raw: &str) -> Result<Vec<CalendarEvent>, serde_json::Error> {
    let resp: ServiceResponse = serde_json::from_str(raw)?;
    let items = resp.body.and_then(|b| b.items).unwrap_or_default();
    Ok(items.into_iter().filter_map(map_item).collect())
}

fn map_item(item: OwaCalendarItem) -> Option<CalendarEvent> {
    let item_id = item.item_id?;
    let id = item_id.id;
    let change_key = item_id.change_key;
    let start = parse_owa_date(item.start.as_deref()?)?;
    let end = parse_owa_date(item.end.as_deref()?)?;
    let location = item.location.and_then(|l| l.display_name);

    let (join_url, platform) = resolve_join(
        item.join_online_meeting_url.as_deref(),
        location.as_deref(),
        item.preview.as_deref(),
    );

    Some(CalendarEvent {
        id,
        change_key,
        title: item.subject.unwrap_or_else(|| "(без темы)".to_string()),
        start,
        end,
        is_all_day: item.is_all_day_event.unwrap_or(false),
        is_cancelled: item.is_cancelled.unwrap_or(false),
        is_organizer: item.is_organizer.unwrap_or(false),
        organizer: item.organizer.and_then(|o| o.mailbox).and_then(|m| m.name),
        location,
        join_url,
        platform,
        response_type: MeetingResponseType::from_owa(item.response_type.as_deref()),
    })
}

/// Join-link priority: the server's own field, then the location text, then
/// the body preview. `JoinOnlineMeetingUrl` still goes through `safe_url`,
/// since it too is server-controlled.
fn resolve_join(
    online: Option<&str>,
    location: Option<&str>,
    preview: Option<&str>,
) -> (Option<String>, MeetingPlatform) {
    if let Some(u) = online {
        if let Some(hit) = detect_meeting_url(u) {
            return (Some(hit.0), hit.1);
        }
        if let Some(safe) = safe_url(u) {
            return (Some(safe), MeetingPlatform::Generic);
        }
    }
    for field in [location, preview].into_iter().flatten() {
        if let Some(hit) = detect_meeting_url(field) {
            return (Some(hit.0), hit.1);
        }
    }
    (None, MeetingPlatform::Generic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_uses_utc_and_naive_range() {
        let s = parse_owa_date("2026-09-04T00:00:00Z").unwrap();
        let e = parse_owa_date("2026-09-04T23:59:59Z").unwrap();
        let p = calendar_view_payload(s, e);
        // Exchange needs `__type` as the first key of every object — the whole
        // reason we don't use serde_json::Value here.
        assert!(p.starts_with(r#"{"__type":"GetCalendarViewJsonRequest:#Exchange""#));
        assert!(p.contains(r#""Header":{"__type":"JsonRequestHeaders:#Exchange""#));
        assert!(p.contains(r#""Body":{"__type":"GetCalendarViewRequest:#Exchange""#));
        assert!(p.contains(r#""Id":"UTC""#));
        // Naive local(UTC) range bounds — no `Z`/offset suffix on the instants.
        assert!(p.contains(r#""RangeStart":"2026-09-04T00:00:00""#));
        assert!(p.contains(r#""RangeEnd":"2026-09-04T23:59:59""#));
        // And it must still be valid JSON.
        let v: serde_json::Value = serde_json::from_str(&p).expect("valid JSON");
        assert_eq!(v["Body"]["CalendarId"]["BaseFolderId"]["Id"], "calendar");
    }

    #[test]
    fn parses_reference_sample() {
        let raw = r#"{
          "Body": { "Items": [
            {"ItemId":{"Id":"AAA="},"Subject":"Ревью","Start":"2026-09-04T11:00:00",
             "End":"2026-09-04T12:00:00","IsAllDayEvent":false,"IsCancelled":false,
             "IsOrganizer":false,"ResponseType":"Accept",
             "Location":{"DisplayName":"https://telemost.yandex.ru/j/12345678"},
             "Organizer":{"Mailbox":{"Name":"Иванов Иван"}},
             "Preview":"текст","JoinOnlineMeetingUrl":null}
          ]}
        }"#;
        let events = parse_calendar_view(raw).unwrap();
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.title, "Ревью");
        assert_eq!(e.start.to_rfc3339(), "2026-09-04T11:00:00+00:00");
        assert_eq!(e.organizer.as_deref(), Some("Иванов Иван"));
        assert_eq!(e.platform, MeetingPlatform::Telemost);
        assert_eq!(e.join_url.as_deref(), Some("https://telemost.yandex.ru/j/12345678"));
        assert_eq!(e.response_type, MeetingResponseType::Accepted);
    }

    #[test]
    fn skips_items_without_id_or_dates() {
        let raw = r#"{"Body":{"Items":[
          {"Subject":"нет id","Start":"2026-09-04T11:00:00","End":"2026-09-04T12:00:00"},
          {"ItemId":{"Id":"B="},"Subject":"нет дат"}
        ]}}"#;
        assert_eq!(parse_calendar_view(raw).unwrap().len(), 0);
    }

    #[test]
    fn empty_body_is_empty_not_error() {
        assert_eq!(parse_calendar_view(r#"{"Body":{}}"#).unwrap().len(), 0);
        assert_eq!(parse_calendar_view(r#"{}"#).unwrap().len(), 0);
    }

    #[test]
    fn event_payload_type_first_and_valid() {
        let p = calendar_event_payload("AA+/=", Some("CK=="));
        assert!(p.starts_with(r#"{"__type":"GetCalendarEventJsonRequest:#Exchange""#));
        assert!(p.contains(r#""EventIds":[{"__type":"ItemId:#Exchange","Id":"AA+/=","ChangeKey":"CK=="}]"#));
        let v: serde_json::Value = serde_json::from_str(&p).expect("valid JSON");
        assert_eq!(v["Body"]["EventIds"][0]["Id"], "AA+/=");
        // Without a change key the field is omitted, still valid JSON.
        let p2 = calendar_event_payload("X", None);
        assert!(!p2.contains("ChangeKey"));
        serde_json::from_str::<serde_json::Value>(&p2).expect("valid JSON");
    }

    #[test]
    fn parses_event_details_attendees_and_body() {
        let raw = r#"{"Body":{"ResponseMessages":[{"ResponseClass":"Success","Items":[{
          "RequiredAttendees":[
            {"Mailbox":{"Name":"Петров Петр","EmailAddress":"p@c.com"},"ResponseType":"Accept"},
            {"Mailbox":{"Name":"Сидоров","EmailAddress":"s@c.com"},"ResponseType":"Tentative"}],
          "OptionalAttendees":[{"Mailbox":{"Name":"Алексеев"},"ResponseType":"NotResponded"}],
          "TextBody":{"Value":"  Коллеги, добрый день!  "},
          "Body":{"Value":"<html><body><p>Привет</p></body></html>"}
        }]}]}}"#;
        let d = parse_event_details(raw).unwrap();
        assert_eq!(d.attendees.len(), 3);
        assert_eq!(d.attendees[0].name, "Петров Петр");
        assert_eq!(d.attendees[0].required, true);
        assert_eq!(d.attendees[0].response, MeetingResponseType::Accepted);
        assert_eq!(d.attendees[2].required, false);
        assert_eq!(d.attendees[2].email, None);
        assert_eq!(d.body_text.as_deref(), Some("Коллеги, добрый день!"));
    }

    #[test]
    fn event_details_falls_back_to_html_body_as_text() {
        let raw = r#"{"Body":{"ResponseMessages":[{"Items":[{
          "Body":{"Value":"<p>Строка&nbsp;один</p><p>два</p>"}}]}]}}"#;
        let d = parse_event_details(raw).unwrap();
        assert_eq!(d.body_text.as_deref(), Some("Строка один два"));
        assert!(d.attendees.is_empty());
    }

    #[test]
    fn rsvp_soap_shape_and_attribute_escaping() {
        let s = rsvp_soap(RsvpAction::Accept, r#"AA"<&"#, "CK'>", true);
        assert!(s.contains(r#"MessageDisposition="SendAndSaveCopy""#));
        assert!(s.contains("<t:AcceptItem>"));
        // Id/ChangeKey are attribute-escaped (quotes and angle brackets).
        assert!(s.contains(r#"Id="AA&quot;&lt;&amp;""#));
        assert!(s.contains(r#"ChangeKey="CK&apos;&gt;""#));

        let silent = rsvp_soap(RsvpAction::Decline, "x", "y", false);
        assert!(silent.contains(r#"MessageDisposition="SaveOnly""#));
        assert!(silent.contains("<t:DeclineItem>"));

        assert!(rsvp_soap(RsvpAction::Tentative, "x", "y", true).contains("<t:TentativelyAcceptItem>"));
    }

    #[test]
    fn ews_ok_and_error_detection() {
        assert!(ews_response_ok("...<m:ResponseCode>NoError</m:ResponseCode>...").is_ok());
        assert!(ews_response_ok(r#"...<m:CreateItemResponseMessage ResponseClass="Success">..."#).is_ok());
        assert_eq!(
            ews_response_ok("<m:ResponseCode>ErrorItemNotFound</m:ResponseCode>").unwrap_err(),
            "ErrorItemNotFound"
        );
    }

    #[test]
    fn username_split_tolerates_stray_slashes_and_space() {
        assert_eq!(split_username(r"MOSCOW\user"), ("MOSCOW".into(), "user".into()));
        // The bug we hit: a doubled slash left "\user" as the user part.
        assert_eq!(split_username(r"MOSCOW\\user"), ("MOSCOW".into(), "user".into()));
        assert_eq!(split_username("  MOSCOW \\ user "), ("MOSCOW".into(), "user".into()));
        assert_eq!(split_username("plainuser"), (String::new(), "plainuser".into()));
    }
}

// ---------------------------------------------------------------------------
// Stateful session: NTLM handshake + GetCalendarView over the pinned agent.
// Network-touching, so no unit tests here — the pure halves above and the
// NTLM crypto in `owa_auth` are what carry the coverage; live verification is
// running the app against a real server.
// ---------------------------------------------------------------------------

use crate::domain::calendar::CalendarError;
use crate::infra::owa_auth;
use crate::infra::owa_http::PinnedConn;
use base64::Engine;

/// One authenticated OWA session, riding a single pinned TLS connection so the
/// NTLM handshake and every subsequent request share one socket (see
/// `infra::owa_http`). Not `Sync`-safe — the service wraps it in a `Mutex`,
/// which also serialises access to the one connection.
pub struct OwaSession {
    host: String,
    domain: String,
    user: String,
    // ponytail: plaintext for the session lifetime (matches the "remember in
    // RAM" decision). Wrap in zeroize::Zeroizing if we ever want scrub-on-drop.
    password: String,
    trusted_cert_pem: Option<String>,
    conn: Option<PinnedConn>,
    canary: Option<String>,
}

/// Splits `DOMAIN\user` into its parts, tolerating a stray doubled slash
/// (`DOMAIN\\user`) and surrounding whitespace — a backslash left on the user
/// part silently corrupts NTOWFv2 and the server rejects Type3 with a fresh
/// challenge. Returns `("", name)` when there is no domain separator.
fn split_username(username: &str) -> (String, String) {
    let t = username.trim();
    match t.split_once('\\') {
        Some((d, u)) => (
            d.trim().trim_matches('\\').to_string(),
            u.trim().trim_matches('\\').to_string(),
        ),
        None => (String::new(), t.to_string()),
    }
}

impl OwaSession {
    /// `username` is `DOMAIN\user`. `base_url` is the Exchange root, no path.
    pub fn new(
        base_url: &str,
        username: &str,
        password: &str,
        trusted_cert_pem: Option<&str>,
    ) -> Result<Self, CalendarError> {
        let (domain, user) = split_username(username);
        let host = url::Url::parse(base_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .ok_or_else(|| CalendarError::Protocol(format!("invalid server URL: {base_url}")))?;
        Ok(Self {
            host,
            domain,
            user,
            password: password.to_string(),
            trusted_cert_pem: trusted_cert_pem.map(str::to_string),
            conn: None,
            canary: None,
        })
    }

    /// Opens a fresh connection and runs the NTLMv2 handshake
    /// (Type1 → Type2 → Type3) on it, then reads `X-OWA-CANARY` from the cookie
    /// the login set. The connection is kept for all later requests.
    pub fn authenticate(&mut self) -> Result<(), CalendarError> {
        self.canary = None;
        let mut conn = PinnedConn::connect(&self.host, self.trusted_cert_pem.as_deref())?;

        // Leg 1: Negotiate → 401 carrying the Type2 challenge.
        let t1 = base64::engine::general_purpose::STANDARD.encode(owa_auth::type1_message());
        let r1 = conn.request("GET", "/owa/", &[("Authorization", &format!("NTLM {t1}"))], &[])?;
        let challenge_hdr = r1.ntlm_challenge().map(str::to_string);
        eprintln!("[calendar] NTLM leg1 HTTP {}, challenge={}", r1.status, challenge_hdr.is_some());

        let challenge_b64 = challenge_hdr
            .and_then(|h| h.strip_prefix("NTLM ").map(str::to_string))
            .ok_or(CalendarError::AuthFailed)?;
        let challenge_bytes = base64::engine::general_purpose::STANDARD
            .decode(challenge_b64.trim())
            .map_err(|e| CalendarError::Protocol(e.to_string()))?;
        let challenge = owa_auth::parse_type2(&challenge_bytes).ok_or(CalendarError::AuthFailed)?;

        // Leg 3: Authenticate on the SAME connection.
        let client_challenge = owa_auth::random_client_challenge();
        let t3 = owa_auth::type3_message(
            &self.user,
            &self.domain,
            &self.password,
            &challenge,
            client_challenge,
        );
        let t3_b64 = base64::engine::general_purpose::STANDARD.encode(&t3);
        if crate::infra::owa_http::debug() {
            let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
            eprintln!("[calendar] dbg user={:?} domain={:?}", self.user, self.domain);
            eprintln!("[calendar] dbg type1_b64={}", base64::engine::general_purpose::STANDARD.encode(owa_auth::type1_message()));
            eprintln!("[calendar] dbg challenge_b64={}", challenge_b64.trim());
            eprintln!("[calendar] dbg server_challenge={}", hex(&challenge.server_challenge));
            eprintln!("[calendar] dbg target_info={}", hex(&challenge.target_info));
            eprintln!("[calendar] dbg client_challenge={}", hex(&client_challenge));
            eprintln!("[calendar] dbg type3_b64={t3_b64}");
        }
        let r3 = conn.request("GET", "/owa/", &[("Authorization", &format!("NTLM {t3_b64}"))], &[])?;
        eprintln!("[calendar] NTLM leg3 HTTP {}", r3.status);
        if r3.status != 200 {
            return Err(CalendarError::AuthFailed);
        }

        self.canary = conn.cookie("X-OWA-CANARY").map(str::to_string);
        self.conn = Some(conn);
        self.canary
            .as_ref()
            .map(|_| ())
            .ok_or_else(|| CalendarError::Protocol("no X-OWA-CANARY cookie after login".into()))
    }

    /// Fetches events in `[start, end)` (UTC). Re-authenticates once on a
    /// session-expiry status (401/440/449) and retries; any second failure
    /// surfaces rather than looping (never hammer the AD account).
    pub fn fetch_calendar_view(
        &mut self,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<CalendarEvent>, CalendarError> {
        if self.conn.is_none() || self.canary.is_none() {
            self.authenticate()?;
        }
        match self.try_fetch(start, end) {
            Err(e) if e.is_retryable() => {
                // Stale/expired connection — drop it, re-auth on a fresh one,
                // retry once.
                self.conn = None;
                self.canary = None;
                self.authenticate()?;
                self.try_fetch(start, end)
            }
            other => other,
        }
    }

    fn try_fetch(
        &mut self,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<CalendarEvent>, CalendarError> {
        let canary = self.canary.clone().ok_or(CalendarError::AuthFailed)?;
        let conn = self.conn.as_mut().ok_or(CalendarError::AuthFailed)?;
        let payload = calendar_view_payload(start, end);
        let encoded: String =
            percent_encoding::utf8_percent_encode(&payload, percent_encoding::NON_ALPHANUMERIC)
                .to_string();

        let resp = conn.request(
            "POST",
            "/owa/service.svc?action=GetCalendarView&EP=1",
            &[
                ("Content-Type", "application/json; charset=UTF-8"),
                ("Action", "GetCalendarView"),
                ("X-Requested-With", "XMLHttpRequest"),
                ("X-OWA-CANARY", &canary),
                ("X-OWA-UrlPostData", &encoded),
            ],
            &[],
        )?;
        eprintln!("[calendar] GetCalendarView HTTP {}, {} bytes", resp.status, resp.body.len());
        // A closed connection is spent — drop it so any next call reconnects
        // rather than writing into a dead socket.
        if resp.connection_close {
            self.conn = None;
            self.canary = None;
        }
        if matches!(resp.status, 401 | 440 | 449) {
            self.canary = None;
            self.conn = None; // force a fresh authenticated connection on retry
            return Err(CalendarError::AuthFailed);
        }
        if !(200..300).contains(&resp.status) {
            return Err(CalendarError::Protocol(format!("HTTP {}", resp.status)));
        }
        parse_calendar_view(&resp.body).map_err(|e| CalendarError::Protocol(e.to_string()))
    }

    /// Fetches attendees + body for one meeting (GetCalendarEvent). Same
    /// re-auth-once behaviour as the view fetch.
    pub fn fetch_event_details(
        &mut self,
        id: &str,
        change_key: Option<&str>,
    ) -> Result<EventDetails, CalendarError> {
        if self.conn.is_none() || self.canary.is_none() {
            self.authenticate()?;
        }
        match self.try_event_details(id, change_key) {
            Err(e) if e.is_retryable() => {
                self.conn = None;
                self.canary = None;
                self.authenticate()?;
                self.try_event_details(id, change_key)
            }
            other => other,
        }
    }

    fn try_event_details(
        &mut self,
        id: &str,
        change_key: Option<&str>,
    ) -> Result<EventDetails, CalendarError> {
        let canary = self.canary.clone().ok_or(CalendarError::AuthFailed)?;
        let conn = self.conn.as_mut().ok_or(CalendarError::AuthFailed)?;
        let body = calendar_event_payload(id, change_key);

        let resp = conn.request(
            "POST",
            "/owa/service.svc?action=GetCalendarEvent&EP=1",
            &[
                ("Content-Type", "application/json; charset=UTF-8"),
                ("Action", "GetCalendarEvent"),
                ("X-Requested-With", "XMLHttpRequest"),
                ("X-OWA-CANARY", &canary),
            ],
            body.as_bytes(),
        )?;
        eprintln!("[calendar] GetCalendarEvent HTTP {}, {} bytes", resp.status, resp.body.len());
        if crate::infra::owa_http::debug() {
            let n = resp.body.len().min(4000);
            eprintln!("[calendar] GetCalendarEvent body[..{n}]: {}", &resp.body[..n]);
        }
        if resp.connection_close {
            self.conn = None;
            self.canary = None;
        }
        if matches!(resp.status, 401 | 440 | 449) {
            self.canary = None;
            self.conn = None;
            return Err(CalendarError::AuthFailed);
        }
        if !(200..300).contains(&resp.status) {
            return Err(CalendarError::Protocol(format!("HTTP {}", resp.status)));
        }
        parse_event_details(&resp.body).map_err(|e| CalendarError::Protocol(e.to_string()))
    }

    /// Sends an RSVP via EWS (`/EWS/Exchange.asmx`) on the authenticated
    /// connection. `send` = notify the organizer vs silent calendar-only.
    /// Re-auth-once like the other calls.
    pub fn rsvp(
        &mut self,
        action: RsvpAction,
        item_id: &str,
        change_key: &str,
        send: bool,
    ) -> Result<(), CalendarError> {
        if self.conn.is_none() || self.canary.is_none() {
            self.authenticate()?;
        }
        match self.try_rsvp(action, item_id, change_key, send) {
            Err(e) if e.is_retryable() => {
                self.conn = None;
                self.canary = None;
                self.authenticate()?;
                self.try_rsvp(action, item_id, change_key, send)
            }
            other => other,
        }
    }

    fn try_rsvp(
        &mut self,
        action: RsvpAction,
        item_id: &str,
        change_key: &str,
        send: bool,
    ) -> Result<(), CalendarError> {
        if self.conn.is_none() {
            self.authenticate()?;
        }
        // /EWS is a separate IIS application from /owa with its own auth — the
        // connection's OWA login does not carry over, so NTLM is handshaked
        // against the EWS endpoint itself (Type1 → challenge → Type3+SOAP).
        let ct = ("Content-Type", "text/xml; charset=utf-8");
        let sa = (
            "SOAPAction",
            "\"http://schemas.microsoft.com/exchange/services/2006/messages/CreateItem\"",
        );
        let soap = rsvp_soap(action, item_id, change_key, send);
        const EWS: &str = "/EWS/Exchange.asmx";

        // Leg 1: Negotiate on /EWS (empty body — the 401 arrives before the
        // EWS handler reads any body).
        let t1 = base64::engine::general_purpose::STANDARD.encode(owa_auth::type1_message());
        let (status1, challenge_hdr, close1) = {
            let conn = self.conn.as_mut().ok_or(CalendarError::AuthFailed)?;
            let r = conn.request("POST", EWS, &[("Authorization", &format!("NTLM {t1}")), ct, sa], &[])?;
            (r.status, r.ntlm_challenge().map(str::to_string), r.connection_close)
        };
        eprintln!("[calendar] EWS leg1 HTTP {status1}, challenge={}", challenge_hdr.is_some());
        if close1 {
            self.conn = None;
            self.canary = None;
            return Err(CalendarError::Network("EWS closed the connection".into()));
        }

        let resp = if let Some(hdr) = challenge_hdr {
            let challenge_b64 = hdr.strip_prefix("NTLM ").ok_or(CalendarError::AuthFailed)?;
            let challenge_bytes = base64::engine::general_purpose::STANDARD
                .decode(challenge_b64.trim())
                .map_err(|e| CalendarError::Protocol(e.to_string()))?;
            let challenge =
                owa_auth::parse_type2(&challenge_bytes).ok_or(CalendarError::AuthFailed)?;
            let t3 = owa_auth::type3_message(
                &self.user,
                &self.domain,
                &self.password,
                &challenge,
                owa_auth::random_client_challenge(),
            );
            let auth = format!("NTLM {}", base64::engine::general_purpose::STANDARD.encode(t3));
            let conn = self.conn.as_mut().ok_or(CalendarError::AuthFailed)?;
            conn.request("POST", EWS, &[("Authorization", &auth), ct, sa], soap.as_bytes())?
        } else if (200..300).contains(&status1) {
            // Connection already authenticated for /EWS — send the real body.
            let conn = self.conn.as_mut().ok_or(CalendarError::AuthFailed)?;
            conn.request("POST", EWS, &[ct, sa], soap.as_bytes())?
        } else {
            return Err(CalendarError::AuthFailed);
        };

        eprintln!("[calendar] EWS RSVP {} HTTP {}", action.ews_element(), resp.status);
        if resp.connection_close {
            self.conn = None;
            self.canary = None;
        }
        if matches!(resp.status, 401 | 440 | 449) {
            return Err(CalendarError::AuthFailed);
        }
        if !(200..300).contains(&resp.status) {
            return Err(CalendarError::Protocol(format!("HTTP {}", resp.status)));
        }
        ews_response_ok(&resp.body).map_err(CalendarError::Protocol)
    }
}
