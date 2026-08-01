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
    label: "Заголовок документа (1 уровень)",
    category: "structure",
    description: "Заголовок первого уровня (=)",
    template: `= Заголовок документа

`,
  },
  {
    id: "doc-attrs",
    label: "Секция оглавления",
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
    label: "Раздел (2 уровень)",
    category: "structure",
    description: "Заголовок второго уровня (==)",
    template: `== Заголовок раздела

Текст раздела.
`,
  },
  {
    id: "subsection",
    label: "Подраздел (3 уровень) со ссылкой",
    category: "structure",
    description: "Заголовок третьего уровня (===)",
    template: `=== Подраздел link:../_external/service/method.adoc[Метод]

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
    id: "job-table",
    label: "Параметры запроса для Job",
    category: "tables",
    description: "Пустая таблица параметров запроса для Job",
    template: `=== Входные параметры

Job не принимает входных параметров.
    
[cols="1,1,1,1"]
|===
| *Параметр* | *Формат* | *Описание* | *Варианты значений*

| Нет входных параметров
| -
| Job запускается по расписанию без входных параметров
|-
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
    id: "http-method",
    label: "Параметры запроса",
    category: "tables",
    description: "Таблица метода, URL и описания",
    template: `== Входные параметры

[cols="1,1,1,1,3,1"]
|===
| *Тип параметра*   | *Параметр* | *Формат* | *Обязательность* | *Описание* | *Варианты значений*

|Метод            5+| POST
|Endpoint         5+| corp-

| Header          
| A-userId     
| string
| required
| X-pin клиента, инициатора запроса
| XAAAAA

| Header          
| A-userIp    
| string
| optional
| Ip-адресс клиента
| 64.233.165.113

| Header          
| A-customerId  
| string
| required
| U-pin клиента, инициатора запроса
| UAAAAA

| Header          
| A-projectId
| string
| required
| Идентификатор приложения инициатора запроса
| WOWTAX

| Header          
| A-clientType
| string
| required
| Тип сервиса инициатора запроса
| FRONT

| Header          
| A-channelId
| string
| required
| Идентификатор вызывающей системы (канала) NIB/ABM/BAAS
| NIB

6+| Тело запроса

| Body          
| -
| -
| required
| -
| -
|===
`,
  },
  {
    id: "response-fields",
    label: "Поля ответа",
    category: "tables",
    description: "Таблица полей ответа",
    template: `== Поля ответа

[cols="1,1,3,1"]
|===
| Параметр | Формат | Описание | Варианты значений

| fieldName
| string
| description
| values
|===
`,
  },
  {
    id: "validation-fields",
    label: "Поля валидации",
    category: "tables",
    description: "Таблица полей валидации",
    template: `== Поля валидации

[cols="1,1,1"]
|===
| Параметр | Условие | Результат 

| param
| condition
| result
|===
`,
  },
  {
    id: "error-codes",
    label: "Коды ошибок",
    category: "tables",
    description: "Таблица кодов и сообщений об ошибках",
    template: `== Коды ошибок

[cols="1,1,2,2"]
|===
| Type | Error Code | Message | Описание

| ValidationException
| validationError
| Some of input parameters are incorrect
| Входные параметры не прошли валидацию
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
    label: "ЗАМЕТКА",
    category: "examples",
    description: "Блок заметки",
    template: `NOTE: Текст заметки.
`,
  },
  {
    id: "tip",
    label: "ПОДСКАЗКА",
    category: "examples",
    description: "Блок подсказки",
    template: `TIP: Полезная подсказка.
`,
  },
  {
    id: "warning",
    label: "ПРЕДУПРЕЖДЕНИЕ",
    category: "examples",
    description: "Блок предупреждения",
    template: `WARNING: Текст предупреждения.
`,
  },
  {
    id: "important",
    label: "ВАЖНО",
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
