//! Localized "what to do" hints for the standards checker, one function per
//! rule id. Mirrors `diagnostic_messages.rs`: messages are plain data, the
//! check logic that decides pass/fail lives in `standards_rules.rs`.

use crate::domain::settings::ErrorLanguage;

/// Shared fallback for any rule that reads the main `methodName.adoc` doc
/// when К.1.1 has already flagged it as missing/unmatched — points back at
/// the root cause instead of restating the rule's own unrelated symptom
/// (e.g. "no :toc:" when the real issue is that no file matched at all).
pub fn main_doc_missing(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => {
            "Основной файл документа не найден — см. К.1.1 (имя файла должно точно совпадать с именем папки метода).".to_string()
        }
        ErrorLanguage::En => {
            "Main document file not found — see K.1.1 (the file name must exactly match the method folder name).".to_string()
        }
    }
}

pub fn k1_1(lang: ErrorLanguage, missing_roles: &[&str]) -> String {
    let joined = missing_roles.join(", ");
    match lang {
        ErrorLanguage::Ru => format!(
            "В папке метода не найдены обязательные файлы: {joined}. Допустимые названия файлов см. в стандарте (К.1.1)."
        ),
        ErrorLanguage::En => format!(
            "Required files are missing in the method folder: {joined}. See the standard (K.1.1) for allowed file names."
        ),
    }
}

pub fn k1_2(lang: ErrorLanguage, ambiguous_roles: &[&str]) -> String {
    let joined = ambiguous_roles.join(", ");
    match lang {
        ErrorLanguage::Ru => format!(
            "Неоднозначная структура папки: более одного файла подходит под роль {joined}. Оставьте по одному файлу на каждую роль."
        ),
        ErrorLanguage::En => format!(
            "Ambiguous folder structure: more than one file matches the {joined} role. Keep exactly one file per role."
        ),
    }
}

pub fn k2_1(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => {
            "В файле метода отсутствует макрос оглавления. Добавьте `:toc:` или `toc::[]`.".to_string()
        }
        ErrorLanguage::En => {
            "The method file is missing the table-of-contents macro. Add `:toc:` or `toc::[]`.".to_string()
        }
    }
}

pub fn k2_2(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "Раздел \"Назначение\"/\"Описание\" отсутствует или короче 50 символов. Добавьте текстовое описание метода.".to_string(),
        ErrorLanguage::En => "The \"Purpose\"/\"Description\" section is missing or shorter than 50 characters. Add a text description of the method.".to_string(),
    }
}

pub fn k3_1(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "Ссылка на diagram-файл отсутствует в документе, либо сам файл диаграммы пуст. Добавьте ссылку и заполните diagram.puml.".to_string(),
        ErrorLanguage::En => "The link to the diagram file is missing from the document, or the diagram file itself is empty. Add the link and fill in diagram.puml.".to_string(),
    }
}

pub fn k4_1(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "Таблица входных параметров не найдена (нужно не менее 4 столбцов). Добавьте таблицу в methodName.adoc или request.adoc.".to_string(),
        ErrorLanguage::En => "Input parameters table not found (needs at least 4 columns). Add a table to methodName.adoc or request.adoc.".to_string(),
    }
}

pub fn k4_2(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "В таблице входных параметров есть пустые ячейки. Заполните все ячейки таблицы.".to_string(),
        ErrorLanguage::En => "The input parameters table has empty cells. Fill in every cell.".to_string(),
    }
}

pub fn k4_3(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "В разделе не приведена ссылка на файл request.adoc из этой же папки. Добавьте include::./request.adoc[] или аналогичную ссылку.".to_string(),
        ErrorLanguage::En => "The section has no link to request.adoc from the same folder. Add include::./request.adoc[] or a similar reference.".to_string(),
    }
}

pub fn k4_4(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "Файл request.adoc пуст либо не содержит endpoint и пример вызова. Добавьте endpoint (/path или ${HOST}/path) и хотя бы один пример вызова.".to_string(),
        ErrorLanguage::En => "request.adoc is empty or has no endpoint and call example. Add an endpoint (/path or ${HOST}/path) and at least one call example.".to_string(),
    }
}

pub fn k5_1(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "Таблица выходных параметров отсутствует. Добавьте таблицу в methodName.adoc или response.adoc, либо укажите фразу-исключение.".to_string(),
        ErrorLanguage::En => "Output parameters table is missing. Add a table to methodName.adoc or response.adoc, or use the exception phrase.".to_string(),
    }
}

pub fn k5_2(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "В таблице выходных параметров есть пустые ячейки. Заполните все ячейки таблицы.".to_string(),
        ErrorLanguage::En => "The output parameters table has empty cells. Fill in every cell.".to_string(),
    }
}

pub fn k5_3(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "В разделе не приведена ссылка на файл response.adoc из этой же папки. Добавьте include::./response.adoc[] или аналогичную ссылку.".to_string(),
        ErrorLanguage::En => "The section has no link to response.adoc from the same folder. Add include::./response.adoc[] or a similar reference.".to_string(),
    }
}

pub fn k5_4(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "Файл response.adoc пуст. Заполните пример ответа.".to_string(),
        ErrorLanguage::En => "response.adoc is empty. Fill in a response example.".to_string(),
    }
}

pub fn k6_1(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "Раздел \"Алгоритм работы\" отсутствует или пуст. Опишите алгоритм работы метода.".to_string(),
        ErrorLanguage::En => "The \"Algorithm\" section is missing or empty. Describe the method's algorithm.".to_string(),
    }
}

pub fn k7_1(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "Раздел \"Обработка ошибок\" отсутствует или пуст. Опишите возможные ошибки и коды ответа.".to_string(),
        ErrorLanguage::En => "The \"Error handling\" section is missing or empty. Describe possible errors and response codes.".to_string(),
    }
}

pub fn passed(lang: ErrorLanguage) -> String {
    match lang {
        ErrorLanguage::Ru => "Требование выполнено.".to_string(),
        ErrorLanguage::En => "Requirement satisfied.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_messages_are_non_empty() {
        for lang in [ErrorLanguage::Ru, ErrorLanguage::En] {
            assert!(!main_doc_missing(lang).is_empty());
            assert!(!k1_1(lang, &["diagram.puml"]).is_empty());
            assert!(!k1_2(lang, &["request"]).is_empty());
            assert!(!k2_1(lang).is_empty());
            assert!(!k2_2(lang).is_empty());
            assert!(!k3_1(lang).is_empty());
            assert!(!k4_1(lang).is_empty());
            assert!(!k4_2(lang).is_empty());
            assert!(!k4_3(lang).is_empty());
            assert!(!k4_4(lang).is_empty());
            assert!(!k5_1(lang).is_empty());
            assert!(!k5_2(lang).is_empty());
            assert!(!k5_3(lang).is_empty());
            assert!(!k5_4(lang).is_empty());
            assert!(!k6_1(lang).is_empty());
            assert!(!k7_1(lang).is_empty());
            assert!(!passed(lang).is_empty());
        }
    }
}
