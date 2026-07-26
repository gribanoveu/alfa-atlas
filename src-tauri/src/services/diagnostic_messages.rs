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
        }
    }
}
