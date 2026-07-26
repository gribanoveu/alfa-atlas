export type AsciiDocSnippetCategory =
  | "structure"
  | "tables"
  | "examples"
  | "includes";

export type AsciiDocSnippet = {
  id: string;
  label: string;
  category: AsciiDocSnippetCategory;
  description?: string;
  template: string;
};

export const ASCIIDOC_SNIPPET_CATEGORIES: {
  id: AsciiDocSnippetCategory;
  label: string;
}[] = [
  { id: "structure", label: "Структура" },
  { id: "tables", label: "Таблицы" },
  { id: "examples", label: "Примеры" },
  { id: "includes", label: "Вставки" },
];

export const ASCIIDOC_SNIPPETS: AsciiDocSnippet[] = [
  {
    id: "doc-title",
    label: "Заголовок документа",
    category: "structure",
    description: "Заголовок первого уровня (=)",
    template: `= Заголовок документа

`,
  },
  {
    id: "doc-attrs",
    label: "Атрибуты документа",
    category: "structure",
    description: "Нумерация разделов и оглавление слева",
    template: `:sectnums:
:sectnumlevels: 3
:toc: left
:toclevels: 3
:toc-title: Оглавление

`,
  },
  {
    id: "section",
    label: "Раздел",
    category: "structure",
    description: "Заголовок второго уровня (==)",
    template: `== Заголовок раздела

Текст раздела.
`,
  },
  {
    id: "subsection",
    label: "Подраздел",
    category: "structure",
    description: "Заголовок третьего уровня (===)",
    template: `=== Подраздел

Текст подраздела.
`,
  },
  {
    id: "anchor",
    label: "Якорь",
    category: "structure",
    description: "[[id]] для ссылок xref",
    template: `[[section-id]]

`,
  },
  {
    id: "ulist",
    label: "Маркированный список",
    category: "structure",
    description: "Список с маркерами (*)",
    template: `* Первый пункт
* Второй пункт
* Третий пункт
`,
  },
  {
    id: "olist",
    label: "Нумерованный список",
    category: "structure",
    description: "Нумерованный список (.)",
    template: `. Первый пункт
. Второй пункт
. Третий пункт
`,
  },
  {
    id: "thematic-break",
    label: "Разделитель",
    category: "structure",
    description: "Горизонтальная линия между блоками",
    template: `'''

`,
  },
  {
    id: "http-method",
    label: "HTTP-метод",
    category: "structure",
    description: "Таблица метода, URL и описания",
    template: `== HTTP-метод

[cols="1,4"]
|===
| Метод | \`POST\`

| URL | \`/api/v1/example\`

| Описание | Краткое описание метода.
|===
`,
  },
  {
    id: "simple-table",
    label: "Таблица",
    category: "tables",
    description: "Простая pipe-таблица",
    template: `[cols="1,1"]
|===
| Колонка A | Колонка B

| Значение 1 | Значение 2
|===
`,
  },
  {
    id: "request-params",
    label: "Параметры запроса",
    category: "tables",
    description: "Таблица параметров запроса",
    template: `== Параметры запроса

[cols="2,1,1,4"]
|===
| Параметр | Тип | Обязательный | Описание

| paramName
| string
| Да
| Описание параметра
|===
`,
  },
  {
    id: "response-fields",
    label: "Поля ответа",
    category: "tables",
    description: "Таблица полей ответа",
    template: `== Поля ответа

[cols="2,1,4"]
|===
| Поле | Тип | Описание

| fieldName
| string
| Описание поля
|===
`,
  },
  {
    id: "error-codes",
    label: "Коды ошибок",
    category: "tables",
    description: "Таблица кодов и сообщений об ошибках",
    template: `== Коды ошибок

[cols="1,4"]
|===
| Код | Описание

| errorCode
| Текст сообщения об ошибке
|===
`,
  },
  {
    id: "source-code",
    label: "Блок кода",
    category: "examples",
    description: "Listing-блок [source]",
    template: `[source]
----
код или команда
----
`,
  },
  {
    id: "source-json",
    label: "JSON-пример",
    category: "examples",
    description: "Listing-блок с JSON",
    template: `[source,json]
----
{
  "example": "value"
}
----
`,
  },
  {
    id: "quote",
    label: "Цитата",
    category: "examples",
    description: "Блок цитаты [quote]",
    template: `[quote]
____
Текст цитаты.
____
`,
  },
  {
    id: "note",
    label: "NOTE",
    category: "examples",
    description: "Блок заметки",
    template: `NOTE: Текст заметки.
`,
  },
  {
    id: "tip",
    label: "TIP",
    category: "examples",
    description: "Блок подсказки",
    template: `TIP: Полезная подсказка.
`,
  },
  {
    id: "warning",
    label: "WARNING",
    category: "examples",
    description: "Блок предупреждения",
    template: `WARNING: Текст предупреждения.
`,
  },
  {
    id: "important",
    label: "IMPORTANT",
    category: "examples",
    description: "Блок важной информации",
    template: `IMPORTANT: Важная информация.
`,
  },
  {
    id: "image",
    label: "Изображение",
    category: "includes",
    description: "image::path.png[]",
    template: `image::images/example.png[Пример, 400, align="center"]
`,
  },
  {
    id: "xref",
    label: "Xref",
    category: "includes",
    description: "Ссылка на другой документ или якорь",
    template: `xref:other.adoc#section-id[Текст ссылки]
`,
  },
  {
    id: "link",
    label: "Ссылка",
    category: "includes",
    description: "Внешняя URL-ссылка",
    template: `https://example.com[Текст ссылки]
`,
  },
  {
    id: "include",
    label: "Include",
    category: "includes",
    description: "include::path/to/file.adoc[]",
    template: `include::path/to/file.adoc[]
`,
  },
];

export function filterSnippets(
  query: string,
  snippets: AsciiDocSnippet[] = ASCIIDOC_SNIPPETS,
): AsciiDocSnippet[] {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return snippets;
  return snippets.filter((snippet) => {
    const haystack = [snippet.label, snippet.description ?? ""]
      .join(" ")
      .toLowerCase();
    return haystack.includes(normalized);
  });
}
