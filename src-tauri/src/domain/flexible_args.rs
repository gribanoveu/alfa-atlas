//! Type coercion for the numeric, boolean and string-list fields of tool
//! arguments — the one place that decides how forgiving `parse_tool_call`
//! is about the *shape* of a value the model sent.
//!
//! Motivation is a real, repeatedly observed failure mode: a model calls
//! `readFile` with `{"path": "...", "startLine": "1", "endLine": "90"}` —
//! every value quoted. The call is semantically perfect and the strings are
//! unambiguous, but a plain `Option<u32>` field rejects it with serde's
//! `invalid type: string "1", expected u32`, the whole call is lost, and
//! the model's usual recovery is to *drop the parameter* rather than fix
//! its type (observed: `readFile` retried verbatim once, then re-sent with
//! no line range at all, reading a 400-line file in full). One transcript
//! spent 6 of its 10 tool errors on exactly this.
//!
//! Quoted scalars are not a model defect worth punishing: the tool schemas
//! go out without provider-side constrained decoding (see
//! `infra::llm_providers::openai_compatible`'s request builder — no
//! `strict` flag), and a JSON-Schema `"type": ["integer", "null"]` union is
//! advisory for a model that is simply generating tokens. So the
//! deserializers here accept the unambiguous spellings and reject only what
//! is genuinely undecidable:
//!
//! - `12`, `"12"`, `" 12 "`, `12.0`, `"12.0"` → `Some(12)`
//! - `null`, omitted, `""`, `"null"`, `"none"`, `"undefined"` → `None`
//! - `true`, `"true"`, `"yes"`, `"1"`, `1` → `Some(true)` (and the
//!   corresponding falsey spellings → `Some(false)`)
//! - `["a", "b"]`, `"a"`, `["a", 12]` → `Some(vec!["a", ...])`; `[]`,
//!   `[""]` → `None`
//! - `"abc"`, `12.5`, `-3`, `[]`, `{}` → an error whose message names the
//!   value and the expected spelling, because guessing here would silently
//!   change what the model asked for.
//!
//! Coercion happens at the edge, so everything downstream still works with
//! honest `u32`/`usize`/`bool` — no `StringOr<T>` leaking into tool
//! implementations.

use serde::de::{Deserializer, Error as DeError};
use serde::Deserialize;
use serde_json::Value;

/// Spellings of "no value" a model uses interchangeably with `null` — an
/// empty string in particular is a common way to say "I'm not setting this
/// optional parameter" when the model is emitting every scalar as a string.
const NULLISH: [&str; 4] = ["", "null", "none", "undefined"];

const TRUTHY: [&str; 4] = ["true", "yes", "1", "y"];
const FALSY: [&str; 4] = ["false", "no", "0", "n"];

/// `Option<u32>` that also accepts a quoted number. Pair with
/// `#[serde(default)]` — `deserialize_with` is called only when the field
/// is *present*, so the default is what covers omission.
pub fn opt_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    opt_uint(deserializer, u32::MAX as u64).map(|v| v.map(|n| n as u32))
}

/// `Option<usize>` counterpart of [`opt_u32`].
pub fn opt_usize<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    opt_uint(deserializer, usize::MAX as u64).map(|v| v.map(|n| n as usize))
}

/// `Option<bool>` that also accepts `"true"` / `"false"` / `"yes"` / `"no"`
/// / `1` / `0`, in any case.
pub fn opt_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else { return Ok(None) };
    match value {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        Value::Number(n) => match n.as_u64() {
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            _ => Err(D::Error::custom(format!(
                "expected true or false, got the number {n} (only 0 and 1 are accepted as numbers)"
            ))),
        },
        Value::String(s) => {
            let lowered = s.trim().to_ascii_lowercase();
            if NULLISH.contains(&lowered.as_str()) {
                return Ok(None);
            }
            if TRUTHY.contains(&lowered.as_str()) {
                return Ok(Some(true));
            }
            if FALSY.contains(&lowered.as_str()) {
                return Ok(Some(false));
            }
            Err(D::Error::custom(format!(
                "expected a boolean, got the string \"{s}\" — send it unquoted as true or false"
            )))
        }
        other => Err(D::Error::custom(format!(
            "expected a boolean, got {}",
            describe(&other)
        ))),
    }
}

/// [`opt_bool`] for a field that is a plain `bool` rather than an
/// `Option<bool>` — an explicit `null` or a nullish string means "not set",
/// which for a flag is the same as `false`.
pub fn flag<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    opt_bool(deserializer).map(|v| v.unwrap_or(false))
}

/// `Option<Vec<String>>` that also accepts a single bare string, and
/// tolerates numbers among the entries.
///
/// Same failure class as a quoted number, one shape up: a model asked for a
/// list of search terms answers `"fts": "уведомления"` about as often as
/// `["уведомления"]`, and a plain `Option<Vec<String>>` rejects the whole
/// call over it. A lone string becomes a one-entry list rather than being
/// split on any separator — the one consumer
/// (`domain::search_query::fts5_query_from_terms`) re-tokenizes entries
/// anyway, so guessing at commas here could only be wrong.
///
/// Blank entries are dropped and an all-blank list reads as absent, so
/// `[]` and `[""]` mean the same thing as omitting the field.
pub fn opt_string_list<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else { return Ok(None) };

    let raw = match value {
        Value::Null => return Ok(None),
        Value::String(s) => {
            if NULLISH.contains(&s.trim().to_ascii_lowercase().as_str()) {
                return Ok(None);
            }
            vec![s]
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(s) => out.push(s),
                    // A year or an error code is a perfectly good search
                    // term; only its spelling is unusual.
                    Value::Number(n) => out.push(n.to_string()),
                    Value::Null => continue,
                    other => {
                        return Err(D::Error::custom(format!(
                            "expected a list of strings, but one entry is {}",
                            describe(&other)
                        )))
                    }
                }
            }
            out
        }
        other => {
            return Err(D::Error::custom(format!(
                "expected a list of strings, got {} — send it as [\"term\", \"term\"]",
                describe(&other)
            )))
        }
    };

    let cleaned: Vec<String> = raw
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Ok((!cleaned.is_empty()).then_some(cleaned))
}

/// Shared body of [`opt_u32`] / [`opt_usize`]: one non-negative integer,
/// spelled as a JSON number or as a string, bounded by `max` so a value
/// that cannot fit the target type is reported as out-of-range rather than
/// silently wrapping.
fn opt_uint<'de, D>(deserializer: D, max: u64) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let Some(value) = value else { return Ok(None) };
    let n = match value {
        Value::Null => return Ok(None),
        Value::Number(ref n) => number_to_uint(&n.to_string()).map_err(|issue| match issue {
            // A JSON number that doesn't parse as one can't happen, but
            // reporting it as "not a number" beats an `unreachable!`.
            NumberIssue::NotANumber => D::Error::custom("expected a number"),
            NumberIssue::Unusable(msg) => D::Error::custom(msg),
        })?,
        Value::String(ref s) => {
            let trimmed = s.trim();
            if NULLISH.contains(&trimmed.to_ascii_lowercase().as_str()) {
                return Ok(None);
            }
            number_to_uint(trimmed).map_err(|issue| match issue {
                NumberIssue::NotANumber => D::Error::custom(format!(
                    "expected a number, got the string \"{s}\" — send it unquoted, e.g. 12"
                )),
                NumberIssue::Unusable(msg) => D::Error::custom(msg),
            })?
        }
        ref other => {
            return Err(D::Error::custom(format!(
                "expected a number, got {}",
                describe(other)
            )))
        }
    };
    if n > max {
        return Err(D::Error::custom(format!(
            "the number {n} is too large for this parameter (maximum {max})"
        )));
    }
    Ok(Some(n))
}

/// Why a literal couldn't become a count. `NotANumber` is worded by the
/// caller (a quoted value gets "send it unquoted", a bare one doesn't);
/// `Unusable` is a real number the parameter can't take, and carries its
/// own explanation.
enum NumberIssue {
    NotANumber,
    Unusable(String),
}

/// Parses one already-trimmed numeric literal — the same routine for a JSON
/// number's own text and for a quoted one, so `12.0` and `"12.0"` cannot
/// disagree. A fractional part is accepted only when it is zero: `12.0` is
/// unambiguously twelve, `12.5` is a real mistake about what the parameter
/// means and rounding it would quietly answer a different question than the
/// model asked.
fn number_to_uint(text: &str) -> Result<u64, NumberIssue> {
    if let Ok(n) = text.parse::<u64>() {
        return Ok(n);
    }
    let Ok(f) = text.parse::<f64>() else {
        return Err(NumberIssue::NotANumber);
    };
    if f.is_nan() || f.is_infinite() {
        return Err(NumberIssue::Unusable(format!(
            "expected a whole number, got {text}"
        )));
    }
    if f < 0.0 {
        return Err(NumberIssue::Unusable(format!(
            "expected a non-negative whole number, got {text}"
        )));
    }
    if f.fract() != 0.0 {
        return Err(NumberIssue::Unusable(format!(
            "expected a whole number, got {text} — round it to an integer"
        )));
    }
    if f > u64::MAX as f64 {
        return Err(NumberIssue::Unusable(format!(
            "the number {text} is too large"
        )));
    }
    Ok(f as u64)
}

/// The JSON kind of a value, for an error message aimed at a model rather
/// than at a Rust reader (`serde`'s own `Unexpected` spells these as
/// `map`/`seq`).
fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Probe {
        #[serde(default, deserialize_with = "super::opt_u32")]
        start_line: Option<u32>,
        #[serde(default, deserialize_with = "super::opt_usize")]
        top_k: Option<usize>,
        #[serde(default, deserialize_with = "super::opt_bool")]
        case_insensitive: Option<bool>,
    }

    fn parse(json: &str) -> Result<Probe, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    #[test]
    fn accepts_plain_numbers_and_booleans() {
        assert_eq!(
            parse(r#"{"startLine":1,"topK":12,"caseInsensitive":true}"#).unwrap(),
            Probe { start_line: Some(1), top_k: Some(12), case_insensitive: Some(true) }
        );
    }

    /// The exact shape from the transcript that motivated this module.
    #[test]
    fn accepts_quoted_numbers() {
        assert_eq!(
            parse(r#"{"startLine":"1","topK":"12"}"#).unwrap(),
            Probe { start_line: Some(1), top_k: Some(12), case_insensitive: None }
        );
    }

    #[test]
    fn accepts_padded_and_zero_fraction_spellings() {
        assert_eq!(
            parse(r#"{"startLine":" 7 ","topK":"15.0"}"#).unwrap(),
            Probe { start_line: Some(7), top_k: Some(15), case_insensitive: None }
        );
        assert_eq!(parse(r#"{"topK":10.0}"#).unwrap().top_k, Some(10));
    }

    #[test]
    fn treats_null_omitted_and_nullish_strings_as_absent() {
        assert_eq!(
            parse("{}").unwrap(),
            Probe { start_line: None, top_k: None, case_insensitive: None }
        );
        assert_eq!(parse(r#"{"startLine":null,"topK":""}"#).unwrap().top_k, None);
        assert_eq!(parse(r#"{"topK":"null"}"#).unwrap().top_k, None);
        assert_eq!(parse(r#"{"caseInsensitive":""}"#).unwrap().case_insensitive, None);
    }

    #[test]
    fn accepts_string_and_numeric_boolean_spellings() {
        assert_eq!(parse(r#"{"caseInsensitive":"true"}"#).unwrap().case_insensitive, Some(true));
        assert_eq!(parse(r#"{"caseInsensitive":"False"}"#).unwrap().case_insensitive, Some(false));
        assert_eq!(parse(r#"{"caseInsensitive":"yes"}"#).unwrap().case_insensitive, Some(true));
        assert_eq!(parse(r#"{"caseInsensitive":1}"#).unwrap().case_insensitive, Some(true));
        assert_eq!(parse(r#"{"caseInsensitive":0}"#).unwrap().case_insensitive, Some(false));
    }

    /// Coercion must not become guessing: these are genuine mistakes about
    /// what the parameter means, and the error has to say so rather than
    /// invent a value.
    #[test]
    fn rejects_values_that_cannot_be_read_as_a_number() {
        for json in [
            r#"{"topK":"a lot"}"#,
            r#"{"topK":12.5}"#,
            r#"{"topK":-3}"#,
            r#"{"topK":[12]}"#,
            r#"{"topK":{"value":12}}"#,
            r#"{"topK":true}"#,
        ] {
            assert!(parse(json).is_err(), "{json} should not have been accepted");
        }
    }

    #[test]
    fn rejects_a_number_too_large_for_the_target_type() {
        let err = parse(r#"{"startLine":99999999999}"#).unwrap_err();
        assert!(err.contains("too large"), "unexpected message: {err}");
    }

    #[test]
    fn number_errors_tell_the_model_how_to_spell_the_value() {
        let err = parse(r#"{"topK":"twelve"}"#).unwrap_err();
        assert!(err.contains("unquoted"), "unexpected message: {err}");
        assert!(err.contains("twelve"), "error should quote the offending value: {err}");
    }

    #[test]
    fn rejects_an_ambiguous_boolean_string() {
        let err = parse(r#"{"caseInsensitive":"maybe"}"#).unwrap_err();
        assert!(err.contains("boolean"), "unexpected message: {err}");
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct ListProbe {
        #[serde(default, deserialize_with = "super::opt_string_list")]
        fts: Option<Vec<String>>,
    }

    fn parse_list(json: &str) -> Result<Option<Vec<String>>, String> {
        serde_json::from_str::<ListProbe>(json)
            .map(|p| p.fts)
            .map_err(|e| e.to_string())
    }

    #[test]
    fn accepts_a_string_list_and_the_lone_string_a_model_sends_instead() {
        assert_eq!(
            parse_list(r#"{"fts":["уведомление","срок"]}"#).unwrap(),
            Some(vec!["уведомление".to_string(), "срок".to_string()])
        );
        assert_eq!(
            parse_list(r#"{"fts":"уведомление"}"#).unwrap(),
            Some(vec!["уведомление".to_string()])
        );
        // A year or an error code is a fine search term, oddly spelled.
        assert_eq!(
            parse_list(r#"{"fts":["ГОСТ",2024]}"#).unwrap(),
            Some(vec!["ГОСТ".to_string(), "2024".to_string()])
        );
    }

    #[test]
    fn treats_an_empty_or_blank_list_as_absent() {
        assert_eq!(parse_list("{}").unwrap(), None);
        assert_eq!(parse_list(r#"{"fts":null}"#).unwrap(), None);
        assert_eq!(parse_list(r#"{"fts":""}"#).unwrap(), None);
        assert_eq!(parse_list(r#"{"fts":[]}"#).unwrap(), None);
        assert_eq!(parse_list(r#"{"fts":["", "  "]}"#).unwrap(), None);
    }

    #[test]
    fn rejects_a_list_shape_that_cannot_be_read_as_terms() {
        let err = parse_list(r#"{"fts":{"term":"a"}}"#).unwrap_err();
        assert!(err.contains("list of strings"), "unexpected message: {err}");
        assert!(parse_list(r#"{"fts":[["nested"]]}"#).is_err());
        assert!(parse_list(r#"{"fts":[true]}"#).is_err());
    }
}
