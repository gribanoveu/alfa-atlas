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

pub const ASCIIDOC_ELEMENT_TEMPLATES: &[AsciidocElementTemplate] = &[
    AsciidocElementTemplate {
        id: "doc-title",
        label: "Заголовок документа (1 уровень)",
        category: "structure",
        description: "Заголовок первого уровня (=)",
        template: "= Заголовок документа\n\n",
    },
    AsciidocElementTemplate {
        id: "doc-attrs",
        label: "Секция оглавления",
        category: "structure",
        description: "Нумерация разделов и оглавление слева",
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
        label: "Параметры запроса",
        category: "tables",
        description: "Таблица метода, URL и описания",
        template: "== Входные параметры\n\n[cols=\"1,1,1,1,3,1\"]\n|===\n| *Тип параметра*   | *Параметр* | *Формат* | *Обязательность* | *Описание* | *Варианты значений*\n\n|Метод            5+| POST\n|Endpoint         5+| corp-\n\n| Header          \n| A-userId     \n| string\n| required\n| X-pin клиента, инициатора запроса\n| XAAAAA\n\n| Header          \n| A-userIp    \n| string\n| optional\n| Ip-адресс клиента\n| 64.233.165.113\n\n| Header          \n| A-customerId  \n| string\n| required\n| U-pin клиента, инициатора запроса\n| UAAAAA\n\n| Header          \n| A-projectId\n| string\n| required\n| Идентификатор приложения инициатора запроса\n| WOWTAX\n\n| Header          \n| A-clientType\n| string\n| required\n| Тип сервиса инициатора запроса\n| FRONT\n\n| Header          \n| A-channelId\n| string\n| required\n| Идентификатор вызывающей системы (канала) NIB/ABM/BAAS\n| NIB\n\n6+| Тело запроса\n\n| Body          \n| -\n| -\n| required\n| -\n| -\n|===\n",
    },
    AsciidocElementTemplate {
        id: "thrift-method",
        label: "Параметры Thrift-запроса",
        category: "tables",
        description: "Таблица стандартного конверта Thrift-запроса (userData) и endpoint",
        template: "== Входные параметры\n\n[cols=\"1,1,1,3\"]\n|===\n| *Параметр* | *Формат* | *Обязательный* | *Описание*\n\n|Endpoint         3+| {host}/<сервис>/tapi\n\n| userData\n| struct\n| да\n| Данные пользователя\n\n| userData.id\n| string\n| да\n| Идентификатор пользователя (xpin/acus)\n\n| userData.authorizedApplicationId\n| string\n| да\n| Идентификатор приложения\n\n| userData.ip\n| string\n| да\n| IP-адрес пользователя\n\n| userData.customerId\n| string\n| да\n| Идентификатор клиента\n|===\n",
    },
    AsciidocElementTemplate {
        id: "response-fields",
        label: "Поля ответа",
        category: "tables",
        description: "Таблица полей ответа",
        template: "== Поля ответа\n\n[cols=\"1,1,3,1\"]\n|===\n| Параметр | Формат | Описание | Варианты значений\n\n| fieldName\n| string\n| description\n| values\n|===\n",
    },
    AsciidocElementTemplate {
        id: "validation-fields",
        label: "Поля валидации",
        category: "tables",
        description: "Таблица полей валидации",
        template: "== Поля валидации\n\n[cols=\"1,1,1\"]\n|===\n| Параметр | Условие | Результат \n\n| param\n| condition\n| result\n|===\n",
    },
    AsciidocElementTemplate {
        id: "error-codes",
        label: "Коды ошибок",
        category: "tables",
        description: "Таблица кодов и сообщений об ошибках",
        template: "== Коды ошибок\n\n[cols=\"1,1,2,2\"]\n|===\n| Type | Error Code | Message | Описание\n\n| ValidationException\n| validationError\n| Some of input parameters are incorrect\n| Входные параметры не прошли валидацию\n|===\n",
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
