/// Static catalog of AsciiDoc element templates (tables, admonitions,
/// lists, includes, …) offered to the AI harness via the `getAsciidocTemplates`
/// tool (see `services::ai_tools::tools::asciidoc_templates::get_asciidoc_templates`).
///
/// This is a manual, intentional mirror of the editor-side catalog in
/// `src/lib/asciidocSnippets.ts` (`ASCIIDOC_SNIPPETS`) — id/label/category/
/// description/template must stay in sync by hand when either side changes.
/// That file's own `!`-command palette (`useMonacoCompletions.ts`'s
/// `BANG_COMMANDS`) already hand-mirrors the same content independently, so
/// this is a third copy of an already-duplicated catalog rather than a new
/// kind of drift — accepted because the tool-calling loop runs server-side
/// in Rust and has no access to the frontend's TS data at all.
pub struct AsciidocElementTemplate {
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub template: &'static str,
}

// Шаблоны разделов постановки. Держатся ровно в том виде, который описан в
// bundled-скиле `method-spec` (`references/structure.md`, `references/errors.md`)
// и проходит проверку стандарта (`services::standards_rules`):
//
// * заголовки того уровня, который задан каркасом документа — таблицы
//   параметров живут на третьем уровне, «Обработка ошибок» на втором;
// * ни одной пустой ячейки в таблицах от четырёх колонок (K.4.2 / K.5.2),
//   вместо пустой ячейки — дефис;
// * строки «Метод» и «Endpoint» на всю ширину через `5+|`, а не четырьмя
//   пустыми ячейками.
//
// Записаны raw-строками, чтобы совпадать символ в символ с шаблонами в
// `src/lib/asciidocSnippets.ts` — за этим следит тест `mirrors_the_editor_catalog`.

const HTTP_METHOD: &str = r#"=== Входные параметры

[cols="1,1,1,1,3,1"]
|===
| *Тип параметра* | *Параметр* | *Формат* | *Обязательность* | *Описание* | *Варианты значений*

|Метод 5+| POST
|Endpoint 5+| https://{host}/<сервис>/<путь>

| Header
| A-userId
| string (length 6 or 9)
| required
| Идентификатор УЛ интернет-банка. Заполняется входным параметром A-userId
| XAAAAA

| Header
| A-customerId
| string (length 6 or 9)
| required
| Идентификатор клиента. Заполняется входным параметром A-customerId
| UAAAAA

| Header
| A-userIp
| string
| optional
| IP УЛ.
| 64.233.165.113

| Header
| A-projectId
| string
| optional
| Идентификатор приложения (модуля приложения) потребителя.
| CORP-<ПРОЕКТ>

| Header
| A-clientType
| string
| required
| Тип сервиса инициатора запроса.
| FRONT

| Header
| A-channelId
| string
| required
| Идентификатор вызывающей системы (канала).
| NIB

6+| Тело запроса

| Body
| fieldName
| string
| required
| Описание поля и чем оно заполняется
| -
|===
"#;

const THRIFT_METHOD: &str = r#"=== Входные параметры

[cols="1,1,1,3"]
|===
| *Параметр* | *Формат* | *Обязательный* | *Описание*

|Endpoint 3+| {host}/<сервис>/tapi

| userData
| struct
| да
| Данные пользователя

| userData.id
| string
| да
| Идентификатор пользователя (xpin/acus)

| userData.authorizedApplicationId
| string
| да
| Идентификатор приложения

| userData.ip
| string
| да
| IP-адрес пользователя

| userData.customerId
| string
| да
| Идентификатор клиента

| fieldName
| string
| да
| Описание поля и чем оно заполняется
|===
"#;

const RESPONSE_FIELDS: &str = r#"=== Выходные параметры

[cols="1,1,3,1"]
|===
| *Параметр* | *Формат* | *Описание* | *Варианты значений*

| fieldName
| string
| Описание поля и источник значения
| -
|===
"#;

const VALIDATION_FIELDS: &str = r#"=== Валидация входных параметров

[cols="1,2,3"]
|===
| *Параметр* | *Условие* | *Результат*

.2+|A-userId
|Параметр имеет значение null или пусто
|Вернуть исключение с http code 400 (Bad Request), указанием на некорректный параметр - "A-userId не указан", type = VALIDATION_ERROR и code = INCORRECT_CONTRACT
|Длина значения не равна 6 символам либо символы не являются алфавитно-числовыми
|Вернуть исключение с http code 400 (Bad Request), указанием на некорректный параметр - "A-userId должен содержать 6 алфавитно-цифровых символов", type = VALIDATION_ERROR и code = INCORRECT_CONTRACT
|===
"#;

const ERROR_CODES: &str = r#"== Обработка ошибок
Описание приведено по ссылке ниже.

include::../CompositeException.adoc[]

*Коды ошибок*

[cols="2,2,1,1"]
|===
| *Условие* | *Описание* | *Type* | *Code*

| Шаг 1. Валидация. Не указан параметр
| A-userId не указан
| VALIDATION_ERROR
| INCORRECT_CONTRACT
|===
"#;

pub const ASCIIDOC_ELEMENT_TEMPLATES: &[AsciidocElementTemplate] = &[
    AsciidocElementTemplate {
        id: "doc-title",
        label: "Заголовок документа (1 уровень)",
        category: "structure",
        description: "Заголовок первого уровня (=); атрибуты шапки идут следующей строкой",
        // Без пустой строки на конце: она закрыла бы шапку документа, и
        // вставленный следом `doc-attrs` уже не сработал бы (`:toc:` из тела
        // не действует). См. `domain::asciidoc_header`.
        template: "= Заголовок документа\n",
    },
    AsciidocElementTemplate {
        id: "doc-attrs",
        label: "Секция оглавления",
        category: "structure",
        description: "Нумерация разделов и оглавление слева; вставляется сразу под заголовком, без пустой строки",
        template: ":sectnums:\n:sectnumlevels: 3\n:toc: left\n:toclevels: 3\n:toc-title: Оглавление\n\n",
    },
    AsciidocElementTemplate {
        id: "section",
        label: "Раздел (2 уровень)",
        category: "structure",
        description: "Заголовок второго уровня (==)",
        template: "== Заголовок раздела\n\nТекст раздела.\n",
    },
    AsciidocElementTemplate {
        id: "subsection",
        label: "Подраздел (3 уровень) со ссылкой",
        category: "structure",
        description: "Заголовок третьего уровня (===)",
        template: "=== Подраздел link:../_external/service/method.adoc[Метод]\n\nТекст подраздела.\n",
    },
    AsciidocElementTemplate {
        id: "anchor",
        label: "Якорь",
        category: "structure",
        description: "[[id]] для ссылок xref",
        template: "[[section-id]]\n\n",
    },
    AsciidocElementTemplate {
        id: "ulist",
        label: "Маркированный список",
        category: "structure",
        description: "Список с маркерами (*)",
        template: "* Первый пункт\n* Второй пункт\n* Третий пункт\n",
    },
    AsciidocElementTemplate {
        id: "olist",
        label: "Нумерованный список",
        category: "structure",
        description: "Нумерованный список (.)",
        template: ". Первый пункт\n. Второй пункт\n. Третий пункт\n",
    },
    AsciidocElementTemplate {
        id: "thematic-break",
        label: "Разделитель",
        category: "structure",
        description: "Горизонтальная линия между блоками",
        template: "'''\n\n",
    },
    AsciidocElementTemplate {
        id: "job-table",
        label: "Параметры запроса для Job",
        category: "tables",
        description: "Пустая таблица параметров запроса для Job",
        template: "=== Входные параметры\n\nJob не принимает входных параметров.\n    \n[cols=\"1,1,1,1\"]\n|===\n| *Параметр* | *Формат* | *Описание* | *Варианты значений*\n\n| Нет входных параметров\n| -\n| Job запускается по расписанию без входных параметров\n|-\n|===\n",
    },
    AsciidocElementTemplate {
        id: "simple-table",
        label: "Таблица",
        category: "tables",
        description: "Простая pipe-таблица",
        template: "[cols=\"1,1\"]\n|===\n| Колонка A | Колонка B\n\n| Значение 1 | Значение 2\n|===\n",
    },
    AsciidocElementTemplate {
        id: "http-method",
        label: "Входные параметры REST-метода",
        category: "tables",
        description: "Таблица метода, эндпоинта и стандартного блока заголовков A-*",
        template: HTTP_METHOD,
    },
    AsciidocElementTemplate {
        id: "thrift-method",
        label: "Входные параметры Thrift-метода",
        category: "tables",
        description: "Таблица эндпоинта и стандартного конверта userData",
        template: THRIFT_METHOD,
    },
    AsciidocElementTemplate {
        id: "response-fields",
        label: "Выходные параметры",
        category: "tables",
        description: "Таблица полей ответа с источником значения",
        template: RESPONSE_FIELDS,
    },
    AsciidocElementTemplate {
        id: "validation-fields",
        label: "Валидация входных параметров",
        category: "tables",
        description: "Таблица Параметр/Условие/Результат с объединением ячеек (.2+|)",
        template: VALIDATION_FIELDS,
    },
    AsciidocElementTemplate {
        id: "error-codes",
        label: "Обработка ошибок",
        category: "tables",
        description: "Раздел с include CompositeException и таблицей Условие/Описание/Type/Code",
        template: ERROR_CODES,
    },
    AsciidocElementTemplate {
        id: "source-code",
        label: "Блок кода",
        category: "examples",
        description: "Listing-блок [source]",
        template: "[source]\n----\nкод или команда\n----\n",
    },
    AsciidocElementTemplate {
        id: "source-json",
        label: "JSON-пример",
        category: "examples",
        description: "Listing-блок с JSON",
        template: "[source,json]\n----\n{\n  \"example\": \"value\"\n}\n----\n",
    },
    AsciidocElementTemplate {
        id: "quote",
        label: "Цитата",
        category: "examples",
        description: "Блок цитаты [quote]",
        template: "[quote]\n____\nТекст цитаты.\n____\n",
    },
    AsciidocElementTemplate {
        id: "note",
        label: "ЗАМЕТКА",
        category: "examples",
        description: "Блок заметки",
        template: "NOTE: Текст заметки.\n",
    },
    AsciidocElementTemplate {
        id: "tip",
        label: "ПОДСКАЗКА",
        category: "examples",
        description: "Блок подсказки",
        template: "TIP: Полезная подсказка.\n",
    },
    AsciidocElementTemplate {
        id: "warning",
        label: "ПРЕДУПРЕЖДЕНИЕ",
        category: "examples",
        description: "Блок предупреждения",
        template: "WARNING: Текст предупреждения.\n",
    },
    AsciidocElementTemplate {
        id: "important",
        label: "ВАЖНО",
        category: "examples",
        description: "Блок важной информации",
        template: "IMPORTANT: Важная информация.\n",
    },
    AsciidocElementTemplate {
        id: "image",
        label: "Изображение",
        category: "includes",
        description: "image::path.png[]",
        template: "image::images/example.png[Пример, 400, align=\"center\"]\n",
    },
    AsciidocElementTemplate {
        id: "xref",
        label: "Xref",
        category: "includes",
        description: "Ссылка на другой документ или якорь",
        template: "xref:other.adoc#section-id[Текст ссылки]\n",
    },
    AsciidocElementTemplate {
        id: "link",
        label: "Ссылка",
        category: "includes",
        description: "Внешняя URL-ссылка",
        template: "https://example.com[Текст ссылки]\n",
    },
    AsciidocElementTemplate {
        id: "include",
        label: "Include",
        category: "includes",
        description: "include::path/to/file.adoc[]",
        template: "include::path/to/file.adoc[]\n",
    },
];

/// Looks up multiple templates by id, preserving `ids`' order for the found
/// entries. Unknown ids are collected separately so the caller (the
/// `getAsciidocTemplates` tool) can tell the model exactly which ids didn't
/// match instead of silently dropping them.
pub fn find_many(ids: &[String]) -> (Vec<&'static AsciidocElementTemplate>, Vec<String>) {
    let mut found = Vec::new();
    let mut not_found = Vec::new();
    for id in ids {
        match ASCIIDOC_ELEMENT_TEMPLATES.iter().find(|t| t.id == id) {
            Some(template) => found.push(template),
            None => not_found.push(id.clone()),
        }
    }
    (found, not_found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_twenty_six_entries_with_unique_ids() {
        assert_eq!(ASCIIDOC_ELEMENT_TEMPLATES.len(), 26);
        let mut ids: Vec<&str> = ASCIIDOC_ELEMENT_TEMPLATES.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 26);
    }

    /// The editor catalog (`src/lib/asciidocSnippets.ts`) is the same content
    /// kept by hand in TypeScript. Its templates are plain template literals,
    /// so every template here must appear in that file verbatim — which is
    /// exactly what silently stopped being true for the parameter and
    /// error-code tables.
    #[test]
    fn mirrors_the_editor_catalog() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/lib/asciidocSnippets.ts");
        let source = std::fs::read_to_string(&path).expect("editor catalog is readable");
        for entry in ASCIIDOC_ELEMENT_TEMPLATES {
            assert!(
                source.contains(entry.template),
                "template {:?} is missing from {}",
                entry.id,
                path.display()
            );
            assert!(
                source.contains(&format!("id: \"{}\"", entry.id)),
                "id {:?} is missing from the editor catalog",
                entry.id
            );
        }
    }

    /// K.4.2/K.5.2 fail a 4+-column table with any blank cell, and these
    /// templates are what a document starts from.
    #[test]
    fn wide_tables_have_no_blank_cells() {
        for entry in ASCIIDOC_ELEMENT_TEMPLATES {
            for (columns, cells) in tables_of(entry.template) {
                if columns < 4 {
                    continue;
                }
                for cell in cells {
                    assert!(
                        !cell.trim().is_empty(),
                        "blank cell in template {:?}",
                        entry.id
                    );
                }
            }
        }
    }

    /// Section titles the standards checker looks for by name.
    #[test]
    fn section_titles_match_the_document_skeleton() {
        let by_id = |id: &str| {
            ASCIIDOC_ELEMENT_TEMPLATES
                .iter()
                .find(|t| t.id == id)
                .unwrap_or_else(|| panic!("no template {id}"))
                .template
        };
        // K.7.1 looks for «Обработка ошибок»; «Коды ошибок» is the caption of
        // the table inside it, not a heading of its own.
        assert!(by_id("error-codes").starts_with("== Обработка ошибок\n"));
        assert!(by_id("error-codes").contains("*Коды ошибок*"));
        assert!(by_id("error-codes").contains("| *Условие* | *Описание* | *Type* | *Code*"));
        assert!(!by_id("error-codes").contains("ValidationException"));
        for id in [
            "http-method",
            "thrift-method",
            "response-fields",
            "validation-fields",
            "job-table",
        ] {
            assert!(by_id(id).starts_with("=== "), "{id} is not a level-3 section");
        }
        // A trailing blank line here would push `doc-attrs` out of the header.
        assert!(by_id("doc-title").ends_with("документа\n"));
        assert!(by_id("doc-attrs").starts_with(':'));
    }

    /// Column count from the first row, then every cell of each `|===` block.
    fn tables_of(source: &str) -> Vec<(usize, Vec<String>)> {
        let mut out = Vec::new();
        let mut current: Option<Vec<&str>> = None;
        for line in source.lines() {
            if line.trim() == "|===" {
                match current.take() {
                    Some(body) => {
                        let columns = body
                            .iter()
                            .map(|l| l.matches('|').count())
                            .find(|n| *n > 0)
                            .unwrap_or(0);
                        let cells = body
                            .iter()
                            .filter(|l| {
                                let t = l.trim_start();
                                t.starts_with('|') || t.starts_with('.')
                            })
                            .flat_map(|l| l.trim().split('|').skip(1).map(str::to_string))
                            .collect();
                        out.push((columns, cells));
                    }
                    None => current = Some(Vec::new()),
                }
                continue;
            }
            if let Some(body) = current.as_mut() {
                body.push(line);
            }
        }
        out
    }

    #[test]
    fn find_many_splits_found_and_not_found_preserving_order() {
        let ids = vec!["simple-table".to_string(), "nope".to_string(), "note".to_string()];
        let (found, not_found) = find_many(&ids);
        let found_ids: Vec<&str> = found.iter().map(|t| t.id).collect();
        assert_eq!(found_ids, vec!["simple-table", "note"]);
        assert_eq!(not_found, vec!["nope".to_string()]);
    }

    #[test]
    fn find_many_with_empty_ids_returns_empty() {
        let (found, not_found) = find_many(&[]);
        assert!(found.is_empty());
        assert!(not_found.is_empty());
    }
}
