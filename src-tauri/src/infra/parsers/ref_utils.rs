//! Shared `"$ref"` target parsing for `infra/parsers/json.rs` and
//! `infra/parsers/yaml.rs` — both walk their own `Value` type looking for
//! `$ref` string entries, but agree on what a *resolvable, local* target
//! looks like, so this is the one place that logic lives.

/// The file part of a `$ref` target, or `None` if it isn't a local file
/// reference worth turning into a dependency edge: a pure JSON-pointer
/// fragment (`"#/definitions/Foo"` — same-document, no file part) or an
/// absolute URL (`http://`/`https://` — not a repo file) both return `None`.
/// `"./schemas/foo.json#/Bar"` returns `Some("./schemas/foo.json")`.
pub fn ref_file_part(raw: &str) -> Option<&str> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return None;
    }
    let file_part = raw.split('#').next().unwrap_or("");
    if file_part.is_empty() {
        None
    } else {
        Some(file_part)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_ref_with_pointer_keeps_file_part_only() {
        assert_eq!(ref_file_part("./schemas/foo.json#/Bar"), Some("./schemas/foo.json"));
    }

    #[test]
    fn relative_ref_without_pointer() {
        assert_eq!(ref_file_part("../common.yaml"), Some("../common.yaml"));
    }

    #[test]
    fn pure_pointer_has_no_file_part() {
        assert_eq!(ref_file_part("#/definitions/Foo"), None);
    }

    #[test]
    fn absolute_url_is_not_a_local_file() {
        assert_eq!(ref_file_part("https://example.com/schema.json"), None);
        assert_eq!(ref_file_part("http://example.com/schema.json"), None);
    }
}
