use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::bundle::bundled_common_spec_value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GradleToken {
    Ident(String),
    StringLit(String),
    LBrace,
    RBrace,
    LParen,
    RParen,
    Equals,
    Colon,
    Dot,
    Comma,
}

/// Tokenizes a Gradle (Groovy / Kotlin DSL) source file, stripping comments
/// and recognizing string literals, identifiers, and syntax symbols.
pub fn tokenize_gradle(input: &str) -> Vec<GradleToken> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        // Whitespace
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // Line comment: //
        if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            i += 2;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Block comment: /* ... */
        if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            } else {
                i = len;
            }
            continue;
        }

        // String literals: """...""", "...", '...'
        if c == '"' || c == '\'' {
            let quote = c;
            // Check triple quote (Kotlin DSL / Groovy multiline)
            if i + 2 < len && chars[i + 1] == quote && chars[i + 2] == quote {
                i += 3;
                let start = i;
                while i + 2 < len
                    && !(chars[i] == quote && chars[i + 1] == quote && chars[i + 2] == quote)
                {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                tokens.push(GradleToken::StringLit(s));
                if i + 2 < len {
                    i += 3;
                } else {
                    i = len;
                }
                continue;
            } else {
                i += 1;
                let mut s = String::new();
                while i < len && chars[i] != quote {
                    if chars[i] == '\\' && i + 1 < len {
                        i += 1;
                        s.push(chars[i]);
                    } else {
                        s.push(chars[i]);
                    }
                    i += 1;
                }
                if i < len && chars[i] == quote {
                    i += 1;
                }
                tokens.push(GradleToken::StringLit(s));
                continue;
            }
        }

        // Single character punctuation
        match c {
            '{' => {
                tokens.push(GradleToken::LBrace);
                i += 1;
                continue;
            }
            '}' => {
                tokens.push(GradleToken::RBrace);
                i += 1;
                continue;
            }
            '(' => {
                tokens.push(GradleToken::LParen);
                i += 1;
                continue;
            }
            ')' => {
                tokens.push(GradleToken::RParen);
                i += 1;
                continue;
            }
            '=' => {
                tokens.push(GradleToken::Equals);
                i += 1;
                continue;
            }
            ':' => {
                tokens.push(GradleToken::Colon);
                i += 1;
                continue;
            }
            '.' => {
                tokens.push(GradleToken::Dot);
                i += 1;
                continue;
            }
            ',' => {
                tokens.push(GradleToken::Comma);
                i += 1;
                continue;
            }
            _ => {}
        }

        // Identifiers and keywords (including numbers, boolean, etc.)
        if c.is_alphanumeric() || c == '_' || c == '-' || c == '$' {
            let start = i;
            while i < len
                && (chars[i].is_alphanumeric()
                    || chars[i] == '_'
                    || chars[i] == '-'
                    || chars[i] == '$')
            {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            tokens.push(GradleToken::Ident(ident));
            continue;
        }

        i += 1;
    }

    tokens
}

fn parse_bool_token(token: &GradleToken) -> Option<bool> {
    match token {
        GradleToken::Ident(s) | GradleToken::StringLit(s) => {
            if s.eq_ignore_ascii_case("true") {
                Some(true)
            } else if s.eq_ignore_ascii_case("false") {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_str_token(token: &GradleToken) -> Option<String> {
    match token {
        GradleToken::StringLit(s) | GradleToken::Ident(s) => Some(s.clone()),
        _ => None,
    }
}

fn parse_assignment_value_bool(tokens: &[GradleToken], mut pos: usize) -> (Option<bool>, usize) {
    if pos >= tokens.len() {
        return (None, pos);
    }

    // 1. .set(true)
    if tokens[pos] == GradleToken::Dot {
        if pos + 4 < tokens.len() {
            if let GradleToken::Ident(method) = &tokens[pos + 1] {
                if method.eq_ignore_ascii_case("set") && tokens[pos + 2] == GradleToken::LParen {
                    if let Some(b) = parse_bool_token(&tokens[pos + 3]) {
                        if tokens[pos + 4] == GradleToken::RParen {
                            return (Some(b), pos + 5);
                        }
                    }
                }
            }
        }
        return (None, pos + 1);
    }

    // 2. (true)
    if tokens[pos] == GradleToken::LParen {
        if pos + 2 < tokens.len() {
            if let Some(b) = parse_bool_token(&tokens[pos + 1]) {
                if tokens[pos + 2] == GradleToken::RParen {
                    return (Some(b), pos + 3);
                }
            }
        }
        return (None, pos + 1);
    }

    // 3. = true / : true
    if tokens[pos] == GradleToken::Equals || tokens[pos] == GradleToken::Colon {
        pos += 1;
        if pos < tokens.len() {
            if let Some(b) = parse_bool_token(&tokens[pos]) {
                return (Some(b), pos + 1);
            }
        }
        return (None, pos);
    }

    // 4. direct `true` / `false` / `'true'`
    if let Some(b) = parse_bool_token(&tokens[pos]) {
        return (Some(b), pos + 1);
    }

    (None, pos)
}

fn parse_assignment_value_str(tokens: &[GradleToken], mut pos: usize) -> (Option<String>, usize) {
    if pos >= tokens.len() {
        return (None, pos);
    }

    // 1. .set("...")
    if tokens[pos] == GradleToken::Dot {
        if pos + 4 < tokens.len() {
            if let GradleToken::Ident(method) = &tokens[pos + 1] {
                if method.eq_ignore_ascii_case("set") && tokens[pos + 2] == GradleToken::LParen {
                    if let Some(s) = parse_str_token(&tokens[pos + 3]) {
                        if tokens[pos + 4] == GradleToken::RParen {
                            return (Some(s), pos + 5);
                        }
                    }
                }
            }
        }
        return (None, pos + 1);
    }

    // 2. ("...")
    if tokens[pos] == GradleToken::LParen {
        if pos + 2 < tokens.len() {
            if let Some(s) = parse_str_token(&tokens[pos + 1]) {
                if tokens[pos + 2] == GradleToken::RParen {
                    return (Some(s), pos + 3);
                }
            }
        }
        return (None, pos + 1);
    }

    // 3. = "..." / : "..."
    if tokens[pos] == GradleToken::Equals || tokens[pos] == GradleToken::Colon {
        pos += 1;
        if pos < tokens.len() {
            if let Some(s) = parse_str_token(&tokens[pos]) {
                return (Some(s), pos + 1);
            }
        }
        return (None, pos);
    }

    // 4. direct string
    if let Some(s) = parse_str_token(&tokens[pos]) {
        return (Some(s), pos + 1);
    }

    (None, pos)
}

/// Extracted OpenAPI configuration from Gradle scripts (`settings.gradle` / `build.gradle`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GradleOpenApiConfig {
    pub common_params: Option<bool>,
    pub common_spec_jar: Option<String>,
}

/// Parses a Gradle script with scope awareness (recognizing `openApiConfigurer { ... }`
/// block, nested blocks, and root-level assignments).
pub fn parse_gradle_openapi_config(content: &str) -> GradleOpenApiConfig {
    let tokens = tokenize_gradle(content);
    let mut config = GradleOpenApiConfig::default();
    let mut scope_stack: Vec<String> = Vec::new();
    let mut last_ident: Option<String> = None;
    let mut i = 0;

    while i < tokens.len() {
        match &tokens[i] {
            GradleToken::LBrace => {
                let scope_name = last_ident.take().unwrap_or_default();
                scope_stack.push(scope_name);
                i += 1;
                continue;
            }
            GradleToken::RBrace => {
                scope_stack.pop();
                last_ident = None;
                i += 1;
                continue;
            }
            GradleToken::Ident(name) => {
                let is_in_openapi_scope = scope_stack.is_empty()
                    || scope_stack.iter().any(|s| {
                        s.eq_ignore_ascii_case("openApiConfigurer")
                            || s.contains("OpenApi")
                            || s.contains("openapi")
                    });

                // Case: openApiConfigurer.commonParams = true / openApiConfigurer.commonParams.set(true)
                if name.eq_ignore_ascii_case("openApiConfigurer")
                    && i + 2 < tokens.len()
                    && tokens[i + 1] == GradleToken::Dot
                {
                    if let GradleToken::Ident(prop) = &tokens[i + 2] {
                        if prop.eq_ignore_ascii_case("commonParams") {
                            let (val, next_i) = parse_assignment_value_bool(&tokens, i + 3);
                            if let Some(b) = val {
                                config.common_params = Some(b);
                            }
                            i = next_i;
                            last_ident = None;
                            continue;
                        } else if prop.eq_ignore_ascii_case("commonSpecJar") {
                            let (val, next_i) = parse_assignment_value_str(&tokens, i + 3);
                            if let Some(s) = val {
                                config.common_spec_jar = Some(s);
                            }
                            i = next_i;
                            last_ident = None;
                            continue;
                        }
                    }
                }

                // Case: commonParams = true (inside openApiConfigurer block or at top-level)
                if name.eq_ignore_ascii_case("commonParams") && is_in_openapi_scope {
                    let (val, next_i) = parse_assignment_value_bool(&tokens, i + 1);
                    if let Some(b) = val {
                        config.common_params = Some(b);
                    }
                    i = next_i;
                    last_ident = None;
                    continue;
                }

                // Case: commonSpecJar = "..."
                if name.eq_ignore_ascii_case("commonSpecJar") && is_in_openapi_scope {
                    let (val, next_i) = parse_assignment_value_str(&tokens, i + 1);
                    if let Some(s) = val {
                        config.common_spec_jar = Some(s);
                    }
                    i = next_i;
                    last_ident = None;
                    continue;
                }

                last_ident = Some(name.clone());
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    config
}

/// Checks whether Gradle text sets `commonParams = true` (or Kotlin DSL `commonParams.set(true)`, etc).
pub fn parse_gradle_common_params(content: &str) -> bool {
    parse_gradle_openapi_config(content)
        .common_params
        .unwrap_or(false)
}

/// Checks whether `repo_root` has a `settings.gradle` / `settings.gradle.kts`
/// or `build.gradle` / `build.gradle.kts` file with `commonParams = true`.
pub fn detect_gradle_common_params(repo_root: &Path) -> bool {
    const CANDIDATES: [&str; 4] = [
        "settings.gradle",
        "settings.gradle.kts",
        "build.gradle",
        "build.gradle.kts",
    ];
    for name in CANDIDATES {
        let path = repo_root.join(name);
        if let Ok(content) = fs::read_to_string(&path) {
            if parse_gradle_common_params(&content) {
                return true;
            }
        }
    }
    false
}

/// Returns the 6 default corporate header parameters from the bundled common spec.
pub fn common_header_parameters() -> Vec<Value> {
    let common_spec = bundled_common_spec_value();
    let param_keys = [
        "userId",
        "customerId",
        "projectId",
        "clientType",
        "channelId",
        "userIp",
    ];
    let mut params = Vec::new();
    if let Some(components_params) = common_spec
        .pointer("/components/parameters")
        .and_then(|v| v.as_object())
    {
        for key in param_keys {
            if let Some(param) = components_params.get(key) {
                params.push(param.clone());
            }
        }
    }
    params
}

const HTTP_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// Injects default corporate header parameters into every operation in `document`
/// that does not already declare them (case-insensitive name check).
pub fn inject_common_parameters(document: &mut Value) {
    let common_params = common_header_parameters();
    if common_params.is_empty() {
        return;
    }

    let Some(paths) = document.get_mut("paths").and_then(|p| p.as_object_mut()) else {
        return;
    };

    for (_path_key, path_item_val) in paths.iter_mut() {
        let Some(path_item) = path_item_val.as_object_mut() else {
            continue;
        };

        // Collect header names defined at path-item level
        let path_headers: HashSet<String> = path_item
            .get("parameters")
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let is_header = p
                            .get("in")
                            .and_then(|i| i.as_str())
                            .map(|i| i.eq_ignore_ascii_case("header"))
                            .unwrap_or(false);
                        if is_header {
                            p.get("name")
                                .and_then(|n| n.as_str())
                                .map(|n| n.to_ascii_lowercase())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        for method in HTTP_METHODS {
            if let Some(op_val) = path_item.get_mut(method) {
                if let Some(op) = op_val.as_object_mut() {
                    let mut existing_headers = path_headers.clone();
                    if let Some(op_params) = op.get("parameters").and_then(|p| p.as_array()) {
                        for p in op_params {
                            let is_header = p
                                .get("in")
                                .and_then(|i| i.as_str())
                                .map(|i| i.eq_ignore_ascii_case("header"))
                                .unwrap_or(false);
                            if is_header {
                                if let Some(name) = p.get("name").and_then(|n| n.as_str()) {
                                    existing_headers.insert(name.to_ascii_lowercase());
                                }
                            }
                        }
                    }

                    let op_params = op
                        .entry("parameters")
                        .or_insert_with(|| Value::Array(Vec::new()));
                    if let Some(arr) = op_params.as_array_mut() {
                        for cp in &common_params {
                            if let Some(name) = cp.get("name").and_then(|n| n.as_str()) {
                                if !existing_headers.contains(&name.to_ascii_lowercase()) {
                                    arr.push(cp.clone());
                                    existing_headers.insert(name.to_ascii_lowercase());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
