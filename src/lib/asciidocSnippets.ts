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
    description: "Заголовок первого уровня (=); атрибуты шапки идут следующей строкой",
    // Без пустой строки на конце: она закрыла бы шапку документа, и
    // вставленный следом `doc-attrs` уже не сработал бы (`:toc:` из тела
    // не действует) — это ловит диагностика detachedHeaderAttributes.
    template: `= Заголовок документа
`,
  },
  {
    id: "doc-attrs",
    label: "Секция оглавления",
    category: "structure",
    description:
      "Нумерация разделов и оглавление слева; вставляется сразу под заголовком, без пустой строки",
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
  // Шаблоны разделов постановки ниже держатся ровно в том виде, который описан
  // в bundled-скиле `method-spec` и проходит проверку стандарта: заголовки того
  // уровня, который задан каркасом документа, и ни одной пустой ячейки в
  // таблицах от четырёх колонок (K.4.2/K.5.2 — вместо пустой ячейки дефис).
  // Их зеркала: `src-tauri/src/domain/asciidoc_element_templates.rs` (для AI) и
  // `BANG_COMMANDS` в `useMonacoCompletions.ts` (для `!`-команд) — оба сверяются
  // с этим файлом тестами.
  {
    id: "http-method",
    label: "Входные параметры REST-метода",
    category: "tables",
    description: "Таблица метода, эндпоинта и стандартного блока заголовков A-*",
    template: `=== Входные параметры

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
`,
  },
  {
    id: "thrift-method",
    label: "Входные параметры Thrift-метода",
    category: "tables",
    description: "Таблица эндпоинта и стандартного конверта userData",
    template: `=== Входные параметры

[cols="1,1,1,3"]
|===
| *Параметр* | *Формат* | *Обязательность* | *Описание*

|Endpoint 3+| {host}/<сервис>/tapi

| userData
| struct
| required
| Данные пользователя

| userData.id
| string
| required
| Идентификатор пользователя (xpin/acus)

| userData.authorizedApplicationId
| string
| required
| Идентификатор приложения

| userData.ip
| string
| required
| IP-адрес пользователя

| userData.customerId
| string
| required
| Идентификатор клиента

| fieldName
| string
| required
| Описание поля и чем оно заполняется
|===
`,
  },
  {
    id: "response-fields",
    label: "Выходные параметры",
    category: "tables",
    description: "Таблица полей ответа с источником значения",
    template: `=== Выходные параметры

[cols="1,1,1,1,1"]
|===
| *Параметр* | *Формат* | *Обязательность* | *Описание* | *Варианты значений*

| fieldName
| string
| required
| Описание поля и источник значения
| -
|===
`,
  },
  {
    id: "validation-fields",
    label: "Валидация входных параметров",
    category: "tables",
    description: "Таблица Параметр/Условие/Результат с объединением ячеек (.2+|)",
    template: `=== Валидация входных параметров

[cols="1,2,3"]
|===
| *Параметр* | *Условие* | *Результат*

.2+|A-userId
|Параметр имеет значение null или пусто
|Вернуть исключение с http code 400 (Bad Request), указанием на некорректный параметр - "A-userId не указан", type = VALIDATION_ERROR и code = INCORRECT_CONTRACT
|Длина значения не равна 6 символам либо символы не являются алфавитно-числовыми
|Вернуть исключение с http code 400 (Bad Request), указанием на некорректный параметр - "A-userId должен содержать 6 алфавитно-цифровых символов", type = VALIDATION_ERROR и code = INCORRECT_CONTRACT
|===
`,
  },
  {
    id: "error-codes",
    label: "Обработка ошибок",
    category: "tables",
    description: "Раздел с include CompositeException и таблицей Условие/Описание/Type/Code",
    template: `== Обработка ошибок
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
