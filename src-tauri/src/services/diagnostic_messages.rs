//! Локализация сообщений диагностик.
//!
//! Сообщения формируются на одном из поддерживаемых языков (`ErrorLanguage`)
//! один раз — в `diagnostics::diagnose_one` — и хранятся в `Diagnostic::message`.
//! Язык читается из `GeneralPrefs` через `settings_store` на каждом проходе
//! диагностики, чтобы смена настройки сразу применялась при следующем build.

use crate::domain::settings::ErrorLanguage;

pub fn missing_include(lang: ErrorLanguage, path: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("include указывает на несуществующий файл: {path}"),
        ErrorLanguage::En => format!("include target not found: {path}"),
    }
}

pub fn missing_xref_document(lang: ErrorLanguage, target: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("xref указывает на несуществующий документ: {target}"),
        ErrorLanguage::En => format!("xref target document not found: {target}"),
    }
}

pub fn missing_xref_anchor_same_doc(lang: ErrorLanguage, anchor: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("метка не найдена в документе: #{anchor}"),
        ErrorLanguage::En => format!("anchor not found in document: #{anchor}"),
    }
}

pub fn missing_xref_anchor(lang: ErrorLanguage, target: &str, anchor: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("метка не найдена в {target}: #{anchor}"),
        ErrorLanguage::En => format!("anchor not found in {target}: #{anchor}"),
    }
}

pub fn missing_image(lang: ErrorLanguage, path: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("изображение не найдено: {path}"),
        ErrorLanguage::En => format!("image not found: {path}"),
    }
}

pub fn duplicate_anchor(lang: ErrorLanguage, id: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("метка определена более одного раза: {id}"),
        ErrorLanguage::En => format!("anchor id defined more than once: {id}"),
    }
}

pub fn circular_include(lang: ErrorLanguage, chain: &[String]) -> String {
    let joined = chain.join(" -> ");
    match lang {
        ErrorLanguage::Ru => format!("циклическая цепочка include: {joined}"),
        ErrorLanguage::En => format!("circular include chain: {joined}"),
    }
}

pub fn detached_header_attributes(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => {
            "атрибуты отделены от заголовка пустой строкой — она закрывает шапку документа, \
             и :toc: не действует: оглавление не построится"
                .to_string()
        }
        ErrorLanguage::En => {
            "a blank line separates these attributes from the document title, which ends the \
             document header — :toc: no longer applies and no table of contents is rendered"
                .to_string()
        }
    }
}

pub fn parse_timeout(lang: ErrorLanguage, secs: u64) -> String {
    match lang {
        ErrorLanguage::Ru => format!("превышен таймаут разбора: {secs} с"),
        ErrorLanguage::En => format!("parse timed out after {secs}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_include_differs_by_lang() {
        assert!(missing_include(ErrorLanguage::Ru, "x.adoc").contains("несуществующий"));
        assert!(missing_include(ErrorLanguage::En, "x.adoc").contains("not found"));
    }

    #[test]
    fn all_messages_are_non_empty() {
        for lang in [ErrorLanguage::Ru, ErrorLanguage::En] {
            assert!(!missing_include(lang, "x.adoc").is_empty());
            assert!(!missing_xref_document(lang, "x.adoc").is_empty());
            assert!(!missing_xref_anchor_same_doc(lang, "id").is_empty());
            assert!(!missing_xref_anchor(lang, "x.adoc", "id").is_empty());
            assert!(!missing_image(lang, "x.png").is_empty());
            assert!(!duplicate_anchor(lang, "id").is_empty());
            assert!(!circular_include(lang, &["a.adoc".to_string()]).is_empty());
            assert!(!parse_timeout(lang, 30).is_empty());
            assert!(!detached_header_attributes(lang).is_empty());
        }
    }
}

// --- Правила OpenAPI (`openapi_lint`) --------------------------------------

pub fn oas_no_servers(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => {
            "в спецификации не объявлено ни одного сервера — «Try it out» некуда отправлять запрос"
                .to_string()
        }
        ErrorLanguage::En => "no servers declared — Try it out has nowhere to send a request".to_string(),
    }
}

pub fn oas_relative_server_url(lang: ErrorLanguage, url: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("адрес сервера не абсолютный: {url}"),
        ErrorLanguage::En => format!("server url is not absolute: {url}"),
    }
}

pub fn oas_duplicate_operation_id(
    lang: ErrorLanguage,
    id: &str,
    method: &str,
    path: &str,
) -> String {
    match lang {
        ErrorLanguage::Ru => {
            format!("operationId «{id}» уже занят операцией {method} {path}")
        }
        ErrorLanguage::En => format!("operationId \"{id}\" is already used by {method} {path}"),
    }
}

pub fn oas_missing_operation_id(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => {
            "у операции нет operationId — по нему генерируются клиенты".to_string()
        }
        ErrorLanguage::En => "operation has no operationId — client generators rely on it".to_string(),
    }
}

pub fn oas_missing_description(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "у операции нет ни summary, ни description".to_string(),
        ErrorLanguage::En => "operation has neither summary nor description".to_string(),
    }
}

pub fn oas_missing_tags(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "у операции нет тега — в списке она попадёт в группу «Other»".to_string(),
        ErrorLanguage::En => "operation has no tag — it lands in the \"Other\" group".to_string(),
    }
}

pub fn oas_duplicate_parameter(lang: ErrorLanguage, name: &str, location: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("параметр «{name}» ({location}) объявлен дважды"),
        ErrorLanguage::En => format!("parameter \"{name}\" ({location}) is declared twice"),
    }
}

pub fn oas_parameter_without_schema(lang: ErrorLanguage, name: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("у параметра «{name}» не объявлена схема"),
        ErrorLanguage::En => format!("parameter \"{name}\" has no schema"),
    }
}

pub fn oas_undeclared_path_parameter(lang: ErrorLanguage, name: &str) -> String {
    match lang {
        ErrorLanguage::Ru => {
            format!("путь содержит {{{name}}}, но такого path-параметра нет в parameters")
        }
        ErrorLanguage::En => {
            format!("path template has {{{name}}} but no such path parameter is declared")
        }
    }
}

pub fn oas_unused_path_parameter(lang: ErrorLanguage, name: &str) -> String {
    match lang {
        ErrorLanguage::Ru => {
            format!("path-параметр «{name}» объявлен, но в шаблоне пути его нет")
        }
        ErrorLanguage::En => {
            format!("path parameter \"{name}\" is declared but missing from the path template")
        }
    }
}

pub fn oas_optional_path_parameter(lang: ErrorLanguage, name: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("path-параметр «{name}» должен быть required: true"),
        ErrorLanguage::En => format!("path parameter \"{name}\" must be required: true"),
    }
}

pub fn oas_no_responses(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "у операции не объявлено ни одного ответа".to_string(),
        ErrorLanguage::En => "operation declares no responses".to_string(),
    }
}

pub fn oas_no_success_response(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "нет ни одного успешного ответа (2xx)".to_string(),
        ErrorLanguage::En => "no success response (2xx) declared".to_string(),
    }
}

pub fn oas_no_error_response(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "не описано ни одной ошибки (4xx/5xx)".to_string(),
        ErrorLanguage::En => "no error response (4xx/5xx) described".to_string(),
    }
}

pub fn oas_response_without_description(lang: ErrorLanguage, status: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("у ответа {status} нет description"),
        ErrorLanguage::En => format!("response {status} has no description"),
    }
}

pub fn oas_media_without_schema(lang: ErrorLanguage, subject: &str, media: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("для {subject} ({media}) не объявлена схема"),
        ErrorLanguage::En => format!("no schema declared for {subject} ({media})"),
    }
}

pub fn oas_empty_enum(lang: ErrorLanguage, subject: &str, media: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("пустой enum в схеме {subject} ({media})"),
        ErrorLanguage::En => format!("empty enum in the {subject} schema ({media})"),
    }
}

pub fn oas_request_body_without_content(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "у тела запроса не объявлено ни одного media type".to_string(),
        ErrorLanguage::En => "request body declares no media type".to_string(),
    }
}

pub fn oas_undeclared_security_scheme(lang: ErrorLanguage, id: &str) -> String {
    match lang {
        ErrorLanguage::Ru => {
            format!("схема авторизации «{id}» не объявлена в components.securitySchemes")
        }
        ErrorLanguage::En => {
            format!("security scheme \"{id}\" is not declared in components.securitySchemes")
        }
    }
}

pub fn oas_unresolved_ref(lang: ErrorLanguage, reference: &str, reason: &str) -> String {
    match lang {
        ErrorLanguage::Ru => format!("не удалось разрешить $ref «{reference}»: {reason}"),
        ErrorLanguage::En => format!("could not resolve $ref \"{reference}\": {reason}"),
    }
}

/// Префикс, которым к сообщению правила приписывается операция: находки
/// раскладываются по файлам, а в файле-фрагменте операция одна не всегда.
pub fn oas_operation_prefix(method: &str, path: &str) -> String {
    format!("{method} {path}: ")
}
