//! Правила проверки OpenAPI-спецификации.
//!
//! Считаются по **собранному** документу (`openapi::load_openapi_bundle`),
//! поэтому здесь нет проверок вида «схема объявлена, но не используется»: к
//! этому моменту каждый `$ref` уже подставлен на место, и `components.schemas`
//! знать о своих потребителях нечего. Неразрешённые `$ref` тоже не
//! дублируются — их отдаёт сам резолвер отдельным списком (`RefDiagnostic`).
//!
//! Находка адресуется JSON Pointer'ом в собранном документе; в конкретный файл
//! её превращает карта источников (`SourceRef`) уже в `diagnostics`.

use serde_json::Value;

use crate::domain::settings::ErrorLanguage;
use crate::domain::workspace_index::Severity;
use crate::services::diagnostic_messages as msgs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRef {
    pub path: String,
    pub method: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecFinding {
    /// Стабильный идентификатор правила — для тестов и будущего подавления.
    pub rule: &'static str,
    pub severity: Severity,
    pub message: String,
    /// Адрес в собранном документе.
    pub pointer: String,
    /// Операция, к которой относится находка.
    pub operation: Option<OperationRef>,
}

const HTTP_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

fn escape_pointer_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

pub fn operation_pointer(path: &str, method: &str) -> String {
    format!("/paths/{}/{method}", escape_pointer_segment(path))
}

fn object<'a>(value: Option<&'a Value>) -> Option<&'a serde_json::Map<String, Value>> {
    value.and_then(|v| v.as_object())
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

/// Узел, оставшийся неразрешённым (`{$ref, unresolved: true}`): правила его
/// пропускают, о нём уже сообщил резолвер.
fn is_unresolved_ref(value: &Value) -> bool {
    value.get("$ref").is_some()
        && (value.get("unresolved").and_then(|v| v.as_bool()) == Some(true)
            || value.get("circular").and_then(|v| v.as_bool()) == Some(true))
}

struct Parameter {
    name: String,
    location: String,
    required: bool,
    has_schema: bool,
}

fn parse_parameters(node: &Value) -> Vec<Parameter> {
    let Some(list) = node.get("parameters").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter(|p| p.is_object() && !is_unresolved_ref(p))
        .map(|p| Parameter {
            name: str_field(p, "name").unwrap_or("?").to_string(),
            location: str_field(p, "in").unwrap_or("?").to_string(),
            required: p.get("required").and_then(|v| v.as_bool()).unwrap_or(false),
            has_schema: p.get("schema").is_some(),
        })
        .collect()
}

/// Параметры операции вместе с общими для всего path item: спека объявляет
/// общий `{id}` один раз на все методы ручки, и без слияния правило про
/// path-параметры ругалось бы на исправную спеку.
fn effective_parameters(path_item: &Value, operation: &Value) -> Vec<Parameter> {
    let own = parse_parameters(operation);
    let own_keys: Vec<String> = own
        .iter()
        .map(|p| format!("{}:{}", p.location, p.name))
        .collect();
    let mut result: Vec<Parameter> = parse_parameters(path_item)
        .into_iter()
        .filter(|p| !own_keys.contains(&format!("{}:{}", p.location, p.name)))
        .collect();
    result.extend(own);
    result
}

fn path_template_names(path: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        names.push(rest[open + 1..open + close].to_string());
        rest = &rest[open + close + 1..];
    }
    names
}

fn declared_security_schemes(document: &Value) -> Vec<String> {
    object(document.pointer("/components/securitySchemes"))
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Схемы, которые может потребовать операция. `security` операции перекрывает
/// глобальную целиком, в том числе пустым массивом — это явный отказ от
/// авторизации именно для этой ручки.
fn operation_security_ids(document: &Value, operation: &Value) -> Vec<String> {
    let raw = operation
        .get("security")
        .and_then(|v| v.as_array())
        .or_else(|| document.get("security").and_then(|v| v.as_array()));
    let Some(raw) = raw else { return Vec::new() };
    let mut ids = Vec::new();
    for entry in raw {
        let Some(map) = entry.as_object() else { continue };
        for key in map.keys() {
            if !ids.contains(key) {
                ids.push(key.clone());
            }
        }
    }
    ids
}

pub fn lint(document: &Value, lang: ErrorLanguage) -> Vec<SpecFinding> {
    let mut findings = Vec::new();
    check_servers(document, lang, &mut findings);

    let declared_schemes = declared_security_schemes(document);
    let Some(paths) = object(document.get("paths")) else {
        return findings;
    };

    let mut seen_operation_ids: Vec<(String, String, String)> = Vec::new();

    for (path, path_item) in paths {
        let Some(path_item_map) = path_item.as_object() else { continue };
        for method in HTTP_METHODS {
            let Some(operation) = path_item_map.get(method) else { continue };
            if !operation.is_object() {
                continue;
            }
            let pointer = operation_pointer(path, method);
            let where_ = OperationRef {
                path: path.clone(),
                method: method.to_string(),
            };

            check_operation_id(
                operation,
                &pointer,
                &where_,
                &mut seen_operation_ids,
                lang,
                &mut findings,
            );
            check_metadata(operation, &pointer, &where_, lang, &mut findings);
            check_parameters(path_item, operation, path, &pointer, &where_, lang, &mut findings);
            check_responses(operation, &pointer, &where_, lang, &mut findings);
            check_request_body(operation, &pointer, &where_, lang, &mut findings);
            check_security(
                document,
                operation,
                &declared_schemes,
                &pointer,
                &where_,
                lang,
                &mut findings,
            );
        }
    }

    findings
}

fn check_servers(document: &Value, lang: ErrorLanguage, findings: &mut Vec<SpecFinding>) {
    let urls: Vec<&str> = document
        .get("servers")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|s| s.get("url").and_then(|u| u.as_str()))
                .filter(|u| !u.is_empty())
                .collect()
        })
        .unwrap_or_default();

    if urls.is_empty() {
        findings.push(SpecFinding {
            rule: "no-servers",
            severity: Severity::Warning,
            message: msgs::oas_no_servers(lang),
            pointer: "/servers".to_string(),
            operation: None,
        });
        return;
    }
    for (index, url) in urls.iter().enumerate() {
        let absolute = url.starts_with("http://") || url.starts_with("https://");
        if !absolute && !url.starts_with('{') {
            findings.push(SpecFinding {
                rule: "relative-server-url",
                severity: Severity::Info,
                message: msgs::oas_relative_server_url(lang, url),
                pointer: format!("/servers/{index}/url"),
                operation: None,
            });
        }
    }
}

fn check_operation_id(
    operation: &Value,
    pointer: &str,
    where_: &OperationRef,
    seen: &mut Vec<(String, String, String)>,
    lang: ErrorLanguage,
    findings: &mut Vec<SpecFinding>,
) {
    let Some(id) = str_field(operation, "operationId") else {
        findings.push(SpecFinding {
            rule: "missing-operation-id",
            severity: Severity::Warning,
            message: msgs::oas_missing_operation_id(lang),
            pointer: pointer.to_string(),
            operation: Some(where_.clone()),
        });
        return;
    };
    if let Some((_, method, path)) = seen.iter().find(|(seen_id, _, _)| seen_id == id) {
        findings.push(SpecFinding {
            rule: "duplicate-operation-id",
            severity: Severity::Error,
            message: msgs::oas_duplicate_operation_id(
                lang,
                id,
                &method.to_uppercase(),
                path,
            ),
            pointer: format!("{pointer}/operationId"),
            operation: Some(where_.clone()),
        });
        return;
    }
    seen.push((id.to_string(), where_.method.clone(), where_.path.clone()));
}

fn check_metadata(
    operation: &Value,
    pointer: &str,
    where_: &OperationRef,
    lang: ErrorLanguage,
    findings: &mut Vec<SpecFinding>,
) {
    let has_text = ["summary", "description"]
        .iter()
        .any(|key| str_field(operation, key).is_some_and(|v| !v.trim().is_empty()));
    if !has_text {
        findings.push(SpecFinding {
            rule: "missing-description",
            severity: Severity::Info,
            message: msgs::oas_missing_description(lang),
            pointer: pointer.to_string(),
            operation: Some(where_.clone()),
        });
    }
    let tags = operation
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|t| t.iter().filter(|v| v.is_string()).count())
        .unwrap_or(0);
    if tags == 0 {
        findings.push(SpecFinding {
            rule: "missing-tags",
            severity: Severity::Info,
            message: msgs::oas_missing_tags(lang),
            pointer: pointer.to_string(),
            operation: Some(where_.clone()),
        });
    }
}

fn check_parameters(
    path_item: &Value,
    operation: &Value,
    path: &str,
    pointer: &str,
    where_: &OperationRef,
    lang: ErrorLanguage,
    findings: &mut Vec<SpecFinding>,
) {
    let parameters = effective_parameters(path_item, operation);
    let params_pointer = format!("{pointer}/parameters");

    let mut seen: Vec<String> = Vec::new();
    for parameter in &parameters {
        let key = format!("{}:{}", parameter.location, parameter.name);
        if seen.contains(&key) {
            findings.push(SpecFinding {
                rule: "duplicate-parameter",
                severity: Severity::Error,
                message: msgs::oas_duplicate_parameter(lang, &parameter.name, &parameter.location),
                pointer: params_pointer.clone(),
                operation: Some(where_.clone()),
            });
        }
        seen.push(key);
        if !parameter.has_schema {
            findings.push(SpecFinding {
                rule: "parameter-without-schema",
                severity: Severity::Warning,
                message: msgs::oas_parameter_without_schema(lang, &parameter.name),
                pointer: params_pointer.clone(),
                operation: Some(where_.clone()),
            });
        }
    }

    let declared: Vec<&str> = parameters
        .iter()
        .filter(|p| p.location == "path")
        .map(|p| p.name.as_str())
        .collect();
    let template = path_template_names(path);

    for name in &template {
        if !declared.contains(&name.as_str()) {
            findings.push(SpecFinding {
                rule: "undeclared-path-parameter",
                severity: Severity::Error,
                message: msgs::oas_undeclared_path_parameter(lang, name),
                pointer: params_pointer.clone(),
                operation: Some(where_.clone()),
            });
        }
    }
    for name in &declared {
        if !template.iter().any(|t| t == name) {
            findings.push(SpecFinding {
                rule: "unused-path-parameter",
                severity: Severity::Error,
                message: msgs::oas_unused_path_parameter(lang, name),
                pointer: params_pointer.clone(),
                operation: Some(where_.clone()),
            });
        }
    }
    for parameter in &parameters {
        if parameter.location == "path" && !parameter.required {
            findings.push(SpecFinding {
                rule: "optional-path-parameter",
                severity: Severity::Error,
                message: msgs::oas_optional_path_parameter(lang, &parameter.name),
                pointer: params_pointer.clone(),
                operation: Some(where_.clone()),
            });
        }
    }
}

fn check_responses(
    operation: &Value,
    pointer: &str,
    where_: &OperationRef,
    lang: ErrorLanguage,
    findings: &mut Vec<SpecFinding>,
) {
    let responses_pointer = format!("{pointer}/responses");
    let Some(responses) = object(operation.get("responses")) else {
        findings.push(SpecFinding {
            rule: "no-responses",
            severity: Severity::Error,
            message: msgs::oas_no_responses(lang),
            pointer: responses_pointer,
            operation: Some(where_.clone()),
        });
        return;
    };
    if responses.is_empty() {
        findings.push(SpecFinding {
            rule: "no-responses",
            severity: Severity::Error,
            message: msgs::oas_no_responses(lang),
            pointer: responses_pointer,
            operation: Some(where_.clone()),
        });
        return;
    }

    let codes: Vec<&String> = responses.keys().collect();
    if !codes
        .iter()
        .any(|c| c.starts_with('2') || c.as_str() == "default")
    {
        findings.push(SpecFinding {
            rule: "no-success-response",
            severity: Severity::Warning,
            message: msgs::oas_no_success_response(lang),
            pointer: responses_pointer.clone(),
            operation: Some(where_.clone()),
        });
    }
    if !codes
        .iter()
        .any(|c| c.starts_with('4') || c.starts_with('5') || c.as_str() == "default")
    {
        findings.push(SpecFinding {
            rule: "no-error-response",
            severity: Severity::Info,
            message: msgs::oas_no_error_response(lang),
            pointer: responses_pointer.clone(),
            operation: Some(where_.clone()),
        });
    }

    for (status, response) in responses {
        if !response.is_object() || is_unresolved_ref(response) {
            continue;
        }
        if !str_field(response, "description").is_some_and(|d| !d.trim().is_empty()) {
            findings.push(SpecFinding {
                rule: "response-without-description",
                severity: Severity::Info,
                message: msgs::oas_response_without_description(lang, status),
                pointer: format!("{responses_pointer}/{status}"),
                operation: Some(where_.clone()),
            });
        }
        check_media(
            response.get("content"),
            &format!("{responses_pointer}/{status}/content"),
            &subject_response(lang, status),
            where_,
            lang,
            findings,
        );
    }
}

fn subject_response(lang: ErrorLanguage, status: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("ответа {status}"),
        ErrorLanguage::En => format!("response {status}"),
    }
}

fn subject_request_body(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "тела запроса".to_string(),
        ErrorLanguage::En => "the request body".to_string(),
    }
}

fn check_request_body(
    operation: &Value,
    pointer: &str,
    where_: &OperationRef,
    lang: ErrorLanguage,
    findings: &mut Vec<SpecFinding>,
) {
    let Some(request_body) = operation.get("requestBody") else { return };
    if !request_body.is_object() || is_unresolved_ref(request_body) {
        return;
    }
    let content = object(request_body.get("content"));
    if content.is_none_or(|map| map.is_empty()) {
        findings.push(SpecFinding {
            rule: "request-body-without-content",
            severity: Severity::Error,
            message: msgs::oas_request_body_without_content(lang),
            pointer: format!("{pointer}/requestBody"),
            operation: Some(where_.clone()),
        });
        return;
    }
    check_media(
        request_body.get("content"),
        &format!("{pointer}/requestBody/content"),
        &subject_request_body(lang),
        where_,
        lang,
        findings,
    );
}

fn check_media(
    content: Option<&Value>,
    pointer: &str,
    subject: &str,
    where_: &OperationRef,
    lang: ErrorLanguage,
    findings: &mut Vec<SpecFinding>,
) {
    let Some(map) = object(content) else { return };
    for (media_type, media) in map {
        let media_pointer = format!("{pointer}/{}", escape_pointer_segment(media_type));
        let Some(schema) = media.get("schema") else {
            findings.push(SpecFinding {
                rule: "media-without-schema",
                severity: Severity::Warning,
                message: msgs::oas_media_without_schema(lang, subject, media_type),
                pointer: media_pointer,
                operation: Some(where_.clone()),
            });
            continue;
        };
        if schema
            .get("enum")
            .and_then(|v| v.as_array())
            .is_some_and(|values| values.is_empty())
        {
            findings.push(SpecFinding {
                rule: "empty-enum",
                severity: Severity::Warning,
                message: msgs::oas_empty_enum(lang, subject, media_type),
                pointer: format!("{media_pointer}/schema"),
                operation: Some(where_.clone()),
            });
        }
    }
}

fn check_security(
    document: &Value,
    operation: &Value,
    declared: &[String],
    pointer: &str,
    where_: &OperationRef,
    lang: ErrorLanguage,
    findings: &mut Vec<SpecFinding>,
) {
    for id in operation_security_ids(document, operation) {
        if declared.contains(&id) {
            continue;
        }
        findings.push(SpecFinding {
            rule: "undeclared-security-scheme",
            severity: Severity::Error,
            message: msgs::oas_undeclared_security_scheme(lang, &id),
            pointer: format!("{pointer}/security"),
            operation: Some(where_.clone()),
        });
    }
}

/// Номер строки (1-based), с которой начинается искомый узел в файле-исходнике.
/// Сначала ищем ключ YAML-отображения (`Pet:`, `listPets:`), затем — любое
/// вхождение строки. Точных позиций у нас нет: резолвер работает через
/// `serde_yaml`, который не хранит номера строк, а парсер со спанами ради
/// одной подсветки — заметно дороже текстового поиска по уже прочитанному
/// файлу. Не нашли — возвращаем 1, файл просто откроется с начала.
pub fn find_spec_line(text: &str, keys: &[String]) -> u32 {
    if keys.is_empty() {
        return 1;
    }
    let lines: Vec<&str> = text.lines().collect();
    for key in keys {
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            let unquoted = trimmed
                .trim_start_matches(['\'', '"'])
                .to_string();
            if unquoted.starts_with(key.as_str()) {
                let rest = &unquoted[key.len()..];
                let rest = rest.trim_start_matches(['\'', '"']);
                if rest.starts_with(':') {
                    return index as u32 + 1;
                }
            }
        }
    }
    for key in keys {
        if let Some(index) = lines.iter().position(|line| line.contains(key.as_str())) {
            return index as u32 + 1;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const RU: ErrorLanguage = ErrorLanguage::Ru;

    fn clean() -> Value {
        json!({
            "openapi": "3.0.3",
            "servers": [{ "url": "https://api.example.com" }],
            "components": {
                "securitySchemes": { "bearerAuth": { "type": "http", "scheme": "bearer" } }
            },
            "paths": {
                "/pets/{id}": {
                    "get": {
                        "tags": ["pets"],
                        "operationId": "getPet",
                        "summary": "Питомец по идентификатору",
                        "security": [{ "bearerAuth": [] }],
                        "parameters": [
                            { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                        ],
                        "responses": {
                            "200": {
                                "description": "OK",
                                "content": { "application/json": { "schema": { "type": "object" } } }
                            },
                            "404": { "description": "Не найден" }
                        }
                    }
                }
            }
        })
    }

    fn rules(document: &Value) -> Vec<&'static str> {
        lint(document, RU).into_iter().map(|f| f.rule).collect()
    }

    #[test]
    fn a_well_formed_operation_produces_no_findings() {
        assert_eq!(lint(&clean(), RU), Vec::new());
    }

    #[test]
    fn reports_a_duplicate_operation_id_once_on_the_second_occurrence() {
        let mut doc = clean();
        let operation = doc["paths"]["/pets/{id}"]["get"].clone();
        let mut a = operation.clone();
        a["parameters"] = json!([]);
        a["operationId"] = json!("same");
        let mut b = a.clone();
        b["operationId"] = json!("same");
        doc["paths"] = json!({ "/a": { "get": a }, "/b": { "get": b } });

        let found: Vec<SpecFinding> = lint(&doc, RU)
            .into_iter()
            .filter(|f| f.rule == "duplicate-operation-id")
            .collect();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].operation,
            Some(OperationRef { path: "/b".into(), method: "get".into() })
        );
        assert_eq!(found[0].severity, Severity::Error);
    }

    #[test]
    fn catches_path_template_and_parameter_list_disagreeing_both_ways() {
        let mut missing = clean();
        missing["paths"]["/pets/{id}"]["get"]["parameters"] = json!([]);
        assert!(rules(&missing).contains(&"undeclared-path-parameter"));

        let mut extra = clean();
        let operation = extra["paths"]["/pets/{id}"]["get"].clone();
        extra["paths"] = json!({ "/pets": { "get": operation } });
        assert!(rules(&extra).contains(&"unused-path-parameter"));
    }

    #[test]
    fn a_path_level_parameter_satisfies_the_template() {
        let mut doc = clean();
        doc["paths"]["/pets/{id}"]["parameters"] =
            json!([{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }]);
        doc["paths"]["/pets/{id}"]["get"]["parameters"] = json!([]);
        assert!(!rules(&doc).contains(&"undeclared-path-parameter"));
    }

    #[test]
    fn an_optional_path_parameter_is_an_error() {
        let mut doc = clean();
        doc["paths"]["/pets/{id}"]["get"]["parameters"] =
            json!([{ "name": "id", "in": "path", "schema": { "type": "string" } }]);
        assert!(rules(&doc).contains(&"optional-path-parameter"));
    }

    #[test]
    fn flags_a_security_scheme_that_is_not_declared_in_components() {
        let mut doc = clean();
        doc["components"]["securitySchemes"] = json!({});
        let found: Vec<SpecFinding> = lint(&doc, RU)
            .into_iter()
            .filter(|f| f.rule == "undeclared-security-scheme")
            .collect();
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("bearerAuth"));
    }

    #[test]
    fn flags_missing_responses_and_missing_status_classes() {
        let mut empty = clean();
        empty["paths"] = json!({
            "/x": { "get": { "tags": ["t"], "operationId": "x", "summary": "s", "responses": {} } }
        });
        assert!(rules(&empty).contains(&"no-responses"));

        let mut only_errors = clean();
        only_errors["paths"] = json!({
            "/x": { "get": {
                "tags": ["t"], "operationId": "x", "summary": "s",
                "responses": { "500": { "description": "boom" } }
            } }
        });
        let found = rules(&only_errors);
        assert!(found.contains(&"no-success-response"));
        assert!(!found.contains(&"no-error-response"));
    }

    #[test]
    fn flags_a_media_type_without_a_schema_and_an_empty_enum() {
        let mut doc = clean();
        doc["paths"] = json!({
            "/x": { "post": {
                "tags": ["t"], "operationId": "x", "summary": "s",
                "requestBody": { "content": { "application/json": {} } },
                "responses": {
                    "200": { "description": "ok", "content": { "application/json": { "schema": { "enum": [] } } } },
                    "400": { "description": "bad" }
                }
            } }
        });
        let found = rules(&doc);
        assert!(found.contains(&"media-without-schema"));
        assert!(found.contains(&"empty-enum"));
    }

    #[test]
    fn flags_a_spec_with_no_servers_and_a_relative_server_url() {
        let mut none = clean();
        none["servers"] = json!([]);
        assert!(rules(&none).contains(&"no-servers"));

        let mut relative = clean();
        relative["servers"] = json!([{ "url": "/api/v2" }]);
        assert!(rules(&relative).contains(&"relative-server-url"));

        let mut templated = clean();
        templated["servers"] = json!([{ "url": "{host}/v2" }]);
        assert!(!rules(&templated).contains(&"relative-server-url"));
    }

    #[test]
    fn metadata_gaps_are_reported_at_the_softest_severities() {
        let mut doc = clean();
        doc["paths"] = json!({
            "/x": { "get": { "responses": {
                "200": { "description": "ok" }, "400": { "description": "b" }
            } } }
        });
        let by_rule: Vec<(&str, Severity)> =
            lint(&doc, RU).into_iter().map(|f| (f.rule, f.severity)).collect();
        assert!(by_rule.contains(&("missing-operation-id", Severity::Warning)));
        assert!(by_rule.contains(&("missing-description", Severity::Info)));
        assert!(by_rule.contains(&("missing-tags", Severity::Info)));
    }

    #[test]
    fn finds_the_line_of_a_node_in_the_source_file() {
        let yaml = concat!(
            "openapi: 3.0.3\n",
            "paths:\n",
            "  /pets:\n",
            "    get:\n",
            "      operationId: listPets\n",
            "components:\n",
            "  schemas:\n",
            "    Pet:\n",
            "      type: object\n",
        );
        assert_eq!(find_spec_line(yaml, &["Pet".to_string()]), 8);
        // Ключа нет — падаем на поиск подстроки.
        assert_eq!(find_spec_line(yaml, &["listPets".to_string()]), 5);
        // Порядок ключей задаёт приоритет.
        assert_eq!(
            find_spec_line(yaml, &["Pet".to_string(), "listPets".to_string()]),
            8
        );
        assert_eq!(
            find_spec_line(yaml, &["nope".to_string(), "listPets".to_string()]),
            5
        );
        // Ключ в кавычках.
        assert_eq!(find_spec_line("'200':\n  description: ok\n", &["200".to_string()]), 1);
        // Не нашли или не искали — файл открывается с начала.
        assert_eq!(find_spec_line(yaml, &["missing".to_string()]), 1);
        assert_eq!(find_spec_line(yaml, &[]), 1);
    }
}
