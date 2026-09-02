//! Projecting an artifact onto the AsciiDoc the documentation templates
//! expect. Pure — no I/O, no framework types.
//!
//! This is the *only* renderer. The tool result the model reads and the
//! live preview in the builder both come through here (the frontend calls
//! it over IPC rather than reimplementing it), so what the user approves in
//! the preview is byte-identical to what the assistant receives.
//!
//! Output shapes are copied from the house templates, not invented:
//!   - the input table is the six-column `http-method` element template
//!     (`domain::asciidoc_element_templates`), whose leading «Тип параметра»
//!     column is what lets one table carry path/query/header/body rows;
//!   - the output table is the five-column one from
//!     `src/templates/asciidoc/rest-endpoint/methodName.adoc`;
//!   - `request.adoc` / `response.adoc` reproduce that same folder's files,
//!     `<details>` wrappers and `[discrete#…]` anchors included.
//!
//! The obligation column is `required`/`optional` in every table. The house
//! standard has one spelling (`method-spec`'s `references/structure.md`), and
//! the scaffold templates were brought to it too — the older `Да`/`Нет` form
//! survives only in documents written before that.

use serde::{Deserialize, Serialize};

use super::artifact::{
    ArtifactContent, ErrorSpec, HttpRequestSpec, JiraTicketSpec, ParamSpec, ResponseSpec,
    TicketLink,
};

/// Placeholder for an empty cell — the templates use `-`, and the standards
/// checker's `table_cell_is_filled` counts it as filled, so an incomplete
/// artifact still produces a table that passes К.4.2/К.5.2.
const EMPTY: &str = "-";

/// Rendered AsciiDoc for one HTTP-request artifact. Every field is a
/// ready-to-paste fragment; `request_adoc`/`response_adoc` are whole files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedHttpRequest {
    /// The «Входные параметры» section, six-column house table.
    pub input_params: String,
    /// The «Выходные параметры» section, five-column table.
    pub output_params: String,
    /// `[source,bash]` curl block.
    pub curl: String,
    /// `[source,json]` blocks, one per documented response.
    pub response_examples: String,
    /// The «Возможные ошибки» table.
    pub errors: String,
    /// A complete `request.adoc`.
    pub request_adoc: String,
    /// A complete `response.adoc`.
    pub response_adoc: String,
}

/// Rendered ticket. One field on purpose: unlike an HTTP request — whose
/// pieces land in different sections of a document — a ticket is pasted
/// into Jira's description box whole, so splitting it into fragments would
/// only give the caller a way to paste half of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedJiraTicket {
    /// Jira **wiki markup** — what the `description` field of a Jira
    /// Server/DC issue actually stores, and what the REST v2 API accepts.
    /// Deliberately not Markdown: see `render_jira_ticket`.
    pub wiki: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RenderedArtifact {
    HttpRequest(RenderedHttpRequest),
    JiraTicket(RenderedJiraTicket),
}

pub fn render(content: &ArtifactContent) -> RenderedArtifact {
    match content {
        ArtifactContent::HttpRequest(spec) => {
            RenderedArtifact::HttpRequest(render_http_request(spec))
        }
        ArtifactContent::JiraTicket(spec) => {
            RenderedArtifact::JiraTicket(render_jira_ticket(spec))
        }
    }
}

// ---- Jira ticket -------------------------------------------------------

/// Renders a ticket as **Jira wiki markup**, not Markdown.
///
/// Not a style preference: this instance is Jira Server (API v2), whose
/// `description` field stores wiki markup. Markdown lands there literally —
/// `##` stays visible text and `- [ ]` is never a checkbox. The exact shape
/// below is copied from a real ticket in this tracker, so a generated
/// description sits next to hand-written ones without looking foreign:
///
/// ```text
/// *3. Что входит в задачу*
///  - Реализация флоу оплаты взноса из календаря
///
/// *6. Критерии приемки* *- AC* *(функционал работает)*
///  - Пользователь может отметить взнос оплаченным
/// ```
///
/// Sections are numbered, and the numbers are assigned **after** empty ones
/// are dropped, so they always run 1..n. A fixed numbering with holes
/// ("…6, 8, 9") reads as a mistake, and the number is a within-description
/// reading aid rather than a stable identifier.
pub fn render_jira_ticket(spec: &JiraTicketSpec) -> RenderedJiraTicket {
    let mut out = Sections::default();

    out.prose("Почему задача существует", &spec.why);
    out.prose("Что должно измениться после задачи", &spec.outcome);
    out.bullets("Что входит в задачу", &spec.in_scope);
    out.bullets("Что не входит в задачу", &spec.out_of_scope);
    out.prose("Техническое решение", &spec.solution);
    out.acceptance_criteria(&spec.acceptance_criteria);
    out.bullets(
        "Критерии выполнения - DoD (задача завершена правильно)",
        &spec.definition_of_done,
    );
    out.bullets("Риски", &spec.risks);
    out.links(&spec.links);

    RenderedJiraTicket { wiki: out.finish() }
}

/// Accumulates rendered sections and hands out their numbers. Numbering is
/// the only reason this is a type rather than a chain of free functions:
/// each section has to know how many non-empty ones came before it.
#[derive(Default)]
struct Sections {
    blocks: Vec<String>,
}

impl Sections {
    fn next_number(&self) -> usize {
        self.blocks.len() + 1
    }

    /// `*7. Заголовок*` — a bold run, not a heading. Jira Server has no
    /// heading in this template; the real tickets use bold + a number.
    fn heading(&self, text: &str) -> String {
        format!("*{}. {}*", self.next_number(), text)
    }

    /// Every `push_*` is a no-op for empty input — the
    /// `jira-task-description` skill is explicit that an empty section is
    /// worse than a missing one, and that has to hold for a half-written
    /// ticket too, not only a finished one.
    fn prose(&mut self, heading: &str, body: &str) {
        let body = body.trim();
        if body.is_empty() {
            return;
        }
        let heading = self.heading(heading);
        self.blocks.push(format!("{heading}\n{body}"));
    }

    fn bullets(&mut self, heading: &str, items: &[String]) {
        let lines = bullet_lines(items);
        if lines.is_empty() {
            return;
        }
        let heading = self.heading(heading);
        self.blocks.push(format!("{heading}\n{}", lines.join("\n")));
    }

    /// The one section whose heading is not a single bold run — reproduced
    /// verbatim from the tracker, where it reads
    /// `*6. Критерии приемки* *- AC* *(функционал работает)*`.
    fn acceptance_criteria(&mut self, items: &[String]) {
        let lines = bullet_lines(items);
        if lines.is_empty() {
            return;
        }
        let n = self.next_number();
        self.blocks.push(format!(
            "*{n}. Критерии приемки* *- AC* *(функционал работает)*\n{}",
            lines.join("\n")
        ));
    }

    /// Grouped by type under a bold sub-heading (`*GIT*`, `*FIGMA*`), the
    /// way the tracker's own tickets carry them. A link with a title becomes
    /// a wiki link (`[текст|url]`); without one, the bare URL, which is what
    /// hand-written tickets contain.
    fn links(&mut self, links: &[TicketLink]) {
        let usable: Vec<&TicketLink> = links.iter().filter(|l| !l.url.trim().is_empty()).collect();
        if usable.is_empty() {
            return;
        }

        let mut body = Vec::<String>::new();
        // Untyped links first, as plain lines: there is no sub-heading to
        // file them under, and inventing one would put a label in the
        // ticket that nobody wrote.
        for link in usable.iter().filter(|l| l.kind.trim().is_empty()) {
            body.push(wiki_link(link));
        }
        // Groups keep first-appearance order rather than being sorted —
        // the author's order is meaningful (design before code, usually).
        let mut seen = Vec::<String>::new();
        for link in usable.iter().filter(|l| !l.kind.trim().is_empty()) {
            let kind = flatten(&link.kind).to_uppercase();
            if !seen.contains(&kind) {
                seen.push(kind);
            }
        }
        for kind in seen {
            let group: Vec<String> = usable
                .iter()
                .filter(|l| flatten(&l.kind).to_uppercase() == kind)
                .map(|l| wiki_link(l))
                .collect();
            body.push(format!("*{kind}*\n{}", group.join("\n")));
        }

        let heading = self.heading("Ссылки");
        self.blocks.push(format!("{heading}\n\n{}", body.join("\n\n")));
    }

    fn finish(self) -> String {
        self.blocks.join("\n\n")
    }
}

/// ` - пункт` — leading space and a hyphen, matching the tracker. Jira also
/// accepts `*` for a bullet, but `*` starts a bold run and the two are
/// easy to confuse when a line is edited by hand.
///
/// Blank entries are dropped: the builder's row editor leaves one behind on
/// every "add row" click.
fn bullet_lines(items: &[String]) -> Vec<String> {
    items
        .iter()
        .map(|item| flatten(item))
        .filter(|item| !item.is_empty())
        .map(|item| format!(" - {item}"))
        .collect()
}

fn wiki_link(link: &TicketLink) -> String {
    let url = flatten(&link.url);
    let title = flatten(&link.title);
    if title.is_empty() {
        url
    } else {
        format!("[{title}|{url}]")
    }
}

/// Collapses whitespace so a value typed across lines stays on one bullet
/// or inside one table cell.
fn flatten(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}


pub fn render_http_request(spec: &HttpRequestSpec) -> RenderedHttpRequest {
    RenderedHttpRequest {
        input_params: render_input_params_table(spec),
        output_params: render_output_params_table(spec),
        curl: render_curl(spec),
        response_examples: render_response_examples(spec),
        errors: render_errors_table(spec),
        request_adoc: render_request_adoc(spec),
        response_adoc: render_response_adoc(spec),
    }
}

// ---- cell helpers ------------------------------------------------------

/// One table cell: trimmed, flattened to a single line, `|` escaped so it
/// cannot break out of the row, `-` when empty.
fn cell(value: &str) -> String {
    let flat = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|");
    if flat.is_empty() {
        EMPTY.to_string()
    } else {
        flat
    }
}

/// The one spelling of the obligation column, in every table.
fn obligation(required: bool) -> &'static str {
    if required {
        "required"
    } else {
        "optional"
    }
}

/// `POST`, or `MethodНеЗадан` — never empty, so the spanning row stays filled.
fn method_upper(spec: &HttpRequestSpec) -> String {
    let m = spec.method.trim();
    if m.is_empty() {
        EMPTY.to_string()
    } else {
        m.to_uppercase()
    }
}

/// `{base_url}{path}` with exactly one separating slash, tolerating either
/// side being empty or already carrying one.
pub fn endpoint(spec: &HttpRequestSpec) -> String {
    let base = spec.base_url.trim().trim_end_matches('/');
    let path = spec.path.trim();
    match (base.is_empty(), path.is_empty()) {
        (true, true) => String::new(),
        (true, false) => {
            if path.starts_with('/') {
                path.to_string()
            } else {
                format!("/{path}")
            }
        }
        (false, true) => base.to_string(),
        (false, false) => {
            if path.starts_with('/') {
                format!("{base}{path}")
            } else {
                format!("{base}/{path}")
            }
        }
    }
}

/// Just the request path, always slash-prefixed. Distinct from
/// `endpoint` because the standards checker's К.4.4 looks for a `/`-leading
/// token in `request.adoc` (`looks_like_endpoint` in
/// `services::standards_rules`), which a scheme-prefixed absolute URL is
/// not — and because a reader wants the path stated plainly, not dug out of
/// a curl string. Empty when no path is filled in yet.
pub fn endpoint_path(spec: &HttpRequestSpec) -> String {
    let path = spec.path.trim();
    if path.is_empty() {
        String::new()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

// ---- input parameters --------------------------------------------------

/// Six-column input table, `http-method` element-template shape. Rows are
/// grouped path → query → header, then a spanning «Тело запроса» separator
/// and the body rows — the same order the request itself is read in.
pub fn render_input_params_table(spec: &HttpRequestSpec) -> String {
    let mut out = String::from("=== Входные параметры\n\n[cols=\"1,1,1,1,3,1\"]\n|===\n");
    out.push_str(
        "| *Тип параметра* | *Параметр* | *Формат* | *Обязательность* | *Описание* | *Варианты значений*\n\n",
    );
    out.push_str(&format!("|Метод 5+| {}\n", method_upper(spec)));
    out.push_str(&format!("|Endpoint 5+| {}\n", cell(&endpoint(spec))));

    for (label, params) in [
        ("Path", &spec.path_params),
        ("Query", &spec.query_params),
        ("Header", &spec.headers),
    ] {
        for param in params.iter() {
            out.push('\n');
            out.push_str(&input_row(label, param));
        }
    }

    if let Some(body) = &spec.body {
        out.push_str("\n6+| Тело запроса\n");
        if body.params.is_empty() {
            out.push('\n');
            out.push_str(&input_row("Body", &ParamSpec { required: true, ..Default::default() }));
        } else {
            for param in body.params.iter() {
                out.push('\n');
                out.push_str(&input_row("Body", param));
            }
        }
    }

    out.push_str("|===\n");
    out
}

fn input_row(location: &str, param: &ParamSpec) -> String {
    format!(
        "| {}\n| {}\n| {}\n| {}\n| {}\n| {}\n",
        location,
        cell(&param.name),
        cell(&param.format),
        obligation(param.required),
        cell(&param.description),
        cell(&param.values),
    )
}

// ---- output parameters -------------------------------------------------

/// Five-column output table from `methodName.adoc`. With more than one
/// documented response carrying fields, each group gets a spanning header
/// row naming its status — a single flat table would silently merge the
/// success and error shapes into one list.
pub fn render_output_params_table(spec: &HttpRequestSpec) -> String {
    let with_params: Vec<&ResponseSpec> = spec
        .responses
        .iter()
        .filter(|r| !r.params.is_empty())
        .collect();

    let mut out = String::from("=== Выходные параметры\n\n[cols=\"1,1,1,1,1\"]\n|===\n");
    out.push_str("| *Параметр* | *Формат* | *Обязательность* | *Описание* | *Варианты значений*\n");

    if with_params.is_empty() {
        out.push('\n');
        out.push_str(&output_row(&ParamSpec::default()));
        out.push_str("|===\n");
        return out;
    }

    let label_groups = with_params.len() > 1;
    for response in with_params {
        if label_groups {
            out.push_str(&format!("\n5+| Ответ {}\n", cell(&response.status)));
        }
        for param in response.params.iter() {
            out.push('\n');
            out.push_str(&output_row(param));
        }
    }
    out.push_str("|===\n");
    out
}

fn output_row(param: &ParamSpec) -> String {
    format!(
        "| {}\n| {}\n| {}\n| {}\n| {}\n",
        cell(&param.name),
        cell(&param.format),
        obligation(param.required),
        cell(&param.description),
        cell(&param.values),
    )
}

// ---- curl --------------------------------------------------------------

/// A `[source,bash]` curl block. Path placeholders are substituted with the
/// param's «Варианты значений» example when it has one and left as
/// `{name}` when it doesn't — an unsubstituted placeholder reads as
/// "fill this in", which is more honest than inventing a value.
pub fn render_curl(spec: &HttpRequestSpec) -> String {
    format!(
        "[source,bash,options=\"nowrap\"]\n----\n{}\n----\n",
        curl_command(spec)
    )
}

pub fn curl_command(spec: &HttpRequestSpec) -> String {
    let mut url = endpoint(spec);
    for param in spec.path_params.iter() {
        let example = first_value(&param.values);
        if !example.is_empty() {
            url = url.replace(&format!("{{{}}}", param.name.trim()), &example);
        }
    }

    let query: Vec<String> = spec
        .query_params
        .iter()
        .filter(|p| !p.name.trim().is_empty())
        .map(|p| {
            let value = first_value(&p.values);
            format!("{}={}", p.name.trim(), value)
        })
        .collect();
    if !query.is_empty() {
        let separator = if url.contains('?') { '&' } else { '?' };
        url.push(separator);
        url.push_str(&query.join("&"));
    }

    let method = method_upper(spec);
    let mut lines = vec![format!("curl -X {method} \"{url}\"")];

    if let Some(body) = &spec.body {
        let media = body.media_type.trim();
        if !media.is_empty() {
            lines.push(format!("  -H \"Content-Type: {media}\""));
        }
    }
    for header in spec.headers.iter() {
        let name = header.name.trim();
        if name.is_empty() {
            continue;
        }
        lines.push(format!("  -H \"{}: {}\"", name, first_value(&header.values)));
    }
    if let Some(body) = &spec.body {
        let sample = body.sample.trim();
        if !sample.is_empty() {
            lines.push(format!("  -d '{}'", indent_continuation(sample)));
        }
    }

    lines.join(" \\\n")
}

/// «Варианты значений» is prose that may list alternatives (`NIB/ABM/BAAS`,
/// `INVOICE, ORDER`). For an executable example only the first one is
/// usable.
fn first_value(values: &str) -> String {
    let trimmed = values.trim();
    if trimmed.is_empty() || trimmed == EMPTY {
        return String::new();
    }
    trimmed
        .split(['/', ',', '|'])
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Keeps a multi-line JSON body readable inside the `-d '…'` argument by
/// indenting its continuation lines under the flag.
fn indent_continuation(sample: &str) -> String {
    let mut lines = sample.lines();
    let first = lines.next().unwrap_or("").to_string();
    let rest: Vec<String> = lines.map(|l| format!("  {l}")).collect();
    if rest.is_empty() {
        first
    } else {
        format!("{}\n{}", first, rest.join("\n"))
    }
}

// ---- responses & errors ------------------------------------------------

pub fn render_response_examples(spec: &HttpRequestSpec) -> String {
    let with_samples: Vec<&ResponseSpec> = spec
        .responses
        .iter()
        .filter(|r| !r.sample.trim().is_empty())
        .collect();
    if with_samples.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for response in with_samples {
        let heading = response_heading(response);
        if !heading.is_empty() {
            out.push_str(&format!("{heading}\n\n"));
        }
        out.push_str("[role=\"response-example\"]\n[source,json,options=\"nowrap\"]\n----\n");
        out.push_str(response.sample.trim());
        out.push_str("\n----\n\n");
    }
    out.trim_end().to_string() + "\n"
}

fn response_heading(response: &ResponseSpec) -> String {
    let status = response.status.trim();
    let description = response.description.trim();
    match (status.is_empty(), description.is_empty()) {
        (true, true) => String::new(),
        (true, false) => format!(".{description}"),
        (false, true) => format!(".{status}"),
        (false, false) => format!(".{status} — {description}"),
    }
}

pub fn render_errors_table(spec: &HttpRequestSpec) -> String {
    let mut out = String::from("|===\n| *Код* | *Описание*\n");
    if spec.errors.is_empty() {
        out.push_str(&error_row(&ErrorSpec::default()));
    } else {
        for error in spec.errors.iter() {
            out.push_str(&error_row(error));
        }
    }
    out.push_str("|===\n");
    out
}

fn error_row(error: &ErrorSpec) -> String {
    format!("\n| {}\n| {}\n", cell(&error.code), cell(&error.description))
}

// ---- whole files -------------------------------------------------------

/// A complete `request.adoc`, reproducing the template's collapsible
/// `<details>` wrapper and per-location tables.
pub fn render_request_adoc(spec: &HttpRequestSpec) -> String {
    let mut out = String::from("--\n++++\n<details>\n<summary><b>Пример запроса</b></summary>\n++++\n\n");

    // Stated before anything else: it is what a reader looks for first, and
    // what К.4.4 scans this file for. Skipped rather than faked when no path
    // is filled in — an invented endpoint is worse than a failing check.
    let path = endpoint_path(spec);
    if !path.is_empty() {
        out.push_str("[discrete#endpoint]\n=== Endpoint\n\n");
        out.push_str(&format!("`{} {}`\n", method_upper(spec), path));
        let host = spec.base_url.trim().trim_end_matches('/');
        if !host.is_empty() {
            out.push_str(&format!("\nХост: `{host}`\n"));
        }
        out.push('\n');
    }

    for (anchor, heading, params) in [
        ("path-params", "Параметры пути", &spec.path_params),
        ("query-params", "Параметры строки запроса", &spec.query_params),
        ("header-params", "Заголовки", &spec.headers),
    ] {
        if params.is_empty() {
            continue;
        }
        out.push_str(&format!("[discrete#{anchor}]\n=== {heading}\n"));
        out.push_str(&request_param_table(params));
        out.push('\n');
    }

    if let Some(body) = &spec.body {
        if !body.params.is_empty() {
            out.push_str("[discrete#body-params]\n=== Параметры тела запроса\n");
            out.push_str(&request_param_table(&body.params));
            out.push('\n');
        }
    }

    out.push_str("[discrete#request-example-heading]\n=== Пример запроса\n\n");
    out.push_str(&render_curl(spec));
    out.push('\n');

    out.push_str("[discrete#errors]\n=== Возможные ошибки\n");
    out.push_str(&render_errors_table(spec));

    out.push_str("\n++++\n</details>\n++++\n--\n");
    out
}

/// The five-column table `request.adoc` uses per location — same columns as
/// the output table, no `[cols]` attribute (the
/// template omits it there).
fn request_param_table(params: &[ParamSpec]) -> String {
    let mut out = String::from("|===\n");
    out.push_str("| *Параметр* | *Формат* | *Обязательность* | *Описание* | *Варианты значений*\n");
    for param in params {
        out.push('\n');
        out.push_str(&output_row(param));
    }
    out.push_str("|===\n");
    out
}

/// A complete `response.adoc`. Falls back to an empty `{}` example so the
/// file is never blank — К.5.4 fails an empty `response.adoc`.
pub fn render_response_adoc(spec: &HttpRequestSpec) -> String {
    let mut out =
        String::from("--\n++++\n<details>\n<summary><b>Пример ответа</b></summary>\n++++\n\n");
    let examples = render_response_examples(spec);
    if examples.is_empty() {
        out.push_str("[role=\"response-example\"]\n[source,json,options=\"nowrap\"]\n----\n{}\n----\n");
    } else {
        out.push_str(&examples);
    }
    out.push_str("\n++++\n</details>\n++++\n--\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::artifact::BodySpec;

    fn param(name: &str, format: &str, required: bool, description: &str, values: &str) -> ParamSpec {
        ParamSpec {
            name: name.into(),
            format: format.into(),
            required,
            description: description.into(),
            values: values.into(),
        }
    }

    fn spec() -> HttpRequestSpec {
        HttpRequestSpec {
            method: "post".into(),
            base_url: "https://corp-gateway-test/".into(),
            path: "/api/{organizationId}/documents".into(),
            path_params: vec![param(
                "organizationId",
                "string",
                true,
                "Идентификатор организации",
                "UBBWQQ",
            )],
            query_params: vec![param("type", "string", false, "Тип документа", "INVOICE, ORDER")],
            headers: vec![param("A-userId", "string", true, "X-pin клиента", "XAAAAA")],
            body: Some(BodySpec {
                media_type: "application/json".into(),
                sample: "{\n  \"type\": \"INVOICE\"\n}".into(),
                params: vec![param("type", "string", true, "Тип", "INVOICE")],
            }),
            responses: vec![ResponseSpec {
                status: "200".into(),
                description: "Успех".into(),
                sample: "{\n  \"id\": \"DOC-1\"\n}".into(),
                params: vec![param("id", "string", true, "Идентификатор", "DOC-1")],
            }],
            errors: vec![ErrorSpec {
                code: "400".into(),
                description: "Невалидное тело запроса".into(),
            }],
            notes: None,
        }
    }

    #[test]
    fn endpoint_joins_with_exactly_one_slash() {
        let mut s = spec();
        assert_eq!(endpoint(&s), "https://corp-gateway-test/api/{organizationId}/documents");
        s.path = "api/x".into();
        s.base_url = "https://h".into();
        assert_eq!(endpoint(&s), "https://h/api/x");
        s.base_url = String::new();
        assert_eq!(endpoint(&s), "/api/x");
    }

    #[test]
    fn input_table_carries_every_location_and_the_body_separator() {
        let table = render_input_params_table(&spec());
        assert!(table.starts_with("=== Входные параметры\n"));
        assert!(table.contains("| *Тип параметра* |"));
        assert!(table.contains("|Метод 5+| POST"));
        assert!(table.contains("|Endpoint 5+| https://corp-gateway-test/api/{organizationId}/documents"));
        assert!(table.contains("| Path\n| organizationId\n| string\n| required\n"));
        assert!(table.contains("| Query\n| type\n| string\n| optional\n"));
        assert!(table.contains("| Header\n| A-userId\n"));
        assert!(table.contains("6+| Тело запроса"));
        assert!(table.contains("| Body\n| type\n"));
        assert!(table.trim_end().ends_with("|==="));
    }

    #[test]
    fn input_table_of_an_empty_spec_still_has_filled_cells() {
        // What the standards checker demands (К.4.2): every cell in a
        // >=4-column table non-empty. A blank artifact must not produce a
        // table that fails the check the document is written to pass.
        let table = render_input_params_table(&HttpRequestSpec::default());
        let body = table
            .split("|===")
            .nth(1)
            .expect("table body");
        for line in body.lines().filter(|l| l.trim_start().starts_with('|')) {
            for raw in line.split('|').skip(1) {
                assert!(!raw.trim().is_empty(), "empty cell in line: {line}");
            }
        }
    }

    #[test]
    fn body_without_params_renders_the_placeholder_row() {
        let mut s = spec();
        s.body = Some(BodySpec { params: vec![], ..Default::default() });
        let table = render_input_params_table(&s);
        assert!(table.contains("| Body\n| -\n| -\n| required\n| -\n| -\n"));
    }

    #[test]
    fn no_body_means_no_body_separator() {
        let mut s = spec();
        s.body = None;
        assert!(!render_input_params_table(&s).contains("Тело запроса"));
    }

    #[test]
    fn output_table_uses_the_house_obligation_and_no_group_header_for_one_response() {
        let table = render_output_params_table(&spec());
        assert!(table.contains("| id\n| string\n| required\n| Идентификатор\n| DOC-1\n"));
        assert!(!table.contains("5+| Ответ"));
    }

    #[test]
    fn output_table_groups_multiple_documented_responses() {
        let mut s = spec();
        s.responses.push(ResponseSpec {
            status: "400".into(),
            description: "Ошибка".into(),
            sample: String::new(),
            params: vec![param("code", "string", true, "Код", "-")],
        });
        let table = render_output_params_table(&s);
        assert!(table.contains("5+| Ответ 200"));
        assert!(table.contains("5+| Ответ 400"));
    }

    #[test]
    fn output_table_of_an_empty_spec_is_a_placeholder_row() {
        let table = render_output_params_table(&HttpRequestSpec::default());
        assert!(table.contains("| -\n| -\n| optional\n| -\n| -\n"));
    }

    #[test]
    fn curl_substitutes_path_params_and_appends_query() {
        let curl = curl_command(&spec());
        assert!(curl.starts_with("curl -X POST \"https://corp-gateway-test/api/UBBWQQ/documents?type=INVOICE\""));
        assert!(curl.contains("-H \"Content-Type: application/json\""));
        assert!(curl.contains("-H \"A-userId: XAAAAA\""));
        assert!(curl.contains("-d '{\n    \"type\": \"INVOICE\"\n  }'"));
    }

    #[test]
    fn curl_leaves_a_placeholder_when_no_example_value_exists() {
        let mut s = spec();
        s.path_params[0].values = String::new();
        s.query_params.clear();
        s.body = None;
        assert!(curl_command(&s).contains("/api/{organizationId}/documents"));
    }

    #[test]
    fn curl_appends_query_with_an_ampersand_when_the_path_already_has_one() {
        let mut s = spec();
        s.path = "/api/{organizationId}/documents?draft=true".into();
        assert!(curl_command(&s).contains("?draft=true&type=INVOICE"));
    }

    #[test]
    fn errors_table_falls_back_to_a_filled_placeholder_row() {
        let mut s = spec();
        s.errors.clear();
        assert!(render_errors_table(&s).contains("| -\n| -\n"));
    }

    #[test]
    fn request_adoc_reproduces_the_template_wrapper_and_sections() {
        let adoc = render_request_adoc(&spec());
        assert!(adoc.starts_with("--\n++++\n<details>"));
        assert!(adoc.contains("[discrete#path-params]\n=== Параметры пути"));
        assert!(adoc.contains("[discrete#query-params]"));
        assert!(adoc.contains("[discrete#header-params]"));
        assert!(adoc.contains("[discrete#body-params]"));
        assert!(adoc.contains("[discrete#request-example-heading]"));
        assert!(adoc.contains("[discrete#errors]"));
        assert!(adoc.trim_end().ends_with("++++\n--"));
    }

    /// Mirrors `services::standards_rules::looks_like_endpoint`.
    fn looks_like_endpoint(content: &str) -> bool {
        content.split_whitespace().any(|raw| {
            let t = raw.trim_matches(|c: char| {
                matches!(c, '`' | '"' | '\'' | '(' | ')' | ',' | '.' | ';' | ':' | '[' | ']')
            });
            t.starts_with('/') && t.len() > 1 && !t.starts_with("//")
        })
    }

    #[test]
    fn request_adoc_contains_an_endpoint_token_for_k4_4() {
        // К.4.4 requires request.adoc to be non-empty and to look like it
        // names an endpoint. A scheme-prefixed URL inside the curl line does
        // not satisfy that, which is why the Endpoint section exists.
        let adoc = render_request_adoc(&spec());
        assert!(adoc.contains("[discrete#endpoint]"));
        assert!(adoc.contains("`POST /api/{organizationId}/documents`"));
        assert!(adoc.contains("Хост: `https://corp-gateway-test`"));
        assert!(looks_like_endpoint(&adoc));
    }

    #[test]
    fn request_adoc_omits_the_endpoint_section_when_no_path_is_filled_in() {
        let mut s = spec();
        s.path = String::new();
        assert!(!render_request_adoc(&s).contains("[discrete#endpoint]"));
    }

    #[test]
    fn endpoint_path_is_always_slash_prefixed() {
        let mut s = spec();
        s.path = "api/x".into();
        assert_eq!(endpoint_path(&s), "/api/x");
        s.path = String::new();
        assert_eq!(endpoint_path(&s), "");
    }

    #[test]
    fn request_adoc_skips_sections_with_no_rows() {
        let mut s = spec();
        s.query_params.clear();
        s.headers.clear();
        let adoc = render_request_adoc(&s);
        assert!(!adoc.contains("[discrete#query-params]"));
        assert!(!adoc.contains("[discrete#header-params]"));
        assert!(adoc.contains("[discrete#path-params]"));
    }

    #[test]
    fn response_adoc_is_never_blank() {
        let mut s = spec();
        s.responses.clear();
        let adoc = render_response_adoc(&s);
        assert!(adoc.contains("----\n{}\n----"));
    }

    #[test]
    fn response_examples_label_each_status() {
        let examples = render_response_examples(&spec());
        assert!(examples.contains(".200 — Успех"));
        assert!(examples.contains("[role=\"response-example\"]"));
    }

    #[test]
    fn cell_escapes_pipes_and_flattens_newlines() {
        assert_eq!(cell("a | b"), "a \\| b");
        assert_eq!(cell("first\n  second"), "first second");
        assert_eq!(cell("   "), "-");
    }

    #[test]
    fn first_value_takes_one_alternative_from_a_list() {
        assert_eq!(first_value("NIB/ABM/BAAS"), "NIB");
        assert_eq!(first_value("INVOICE, ORDER"), "INVOICE");
        assert_eq!(first_value("-"), "");
    }

    // ---- Jira ticket ---------------------------------------------------

    fn ticket() -> JiraTicketSpec {
        JiraTicketSpec {
            why: "Бухгалтер не видит, какие взносы уже оплачены.".into(),
            outcome: "Пользователь может отметить взнос оплаченным.".into(),
            in_scope: vec!["Кнопка «Оплачено» в карточке взноса".into()],
            out_of_scope: vec!["Интеграция с банком".into()],
            solution: "Использовать endpoint wlbuh-enp-api/change-payments".into(),
            acceptance_criteria: vec!["Пользователь видит статус без перезагрузки".into()],
            definition_of_done: vec!["Добавлены UI-тесты".into()],
            risks: vec!["Проблема с округлением копеек".into()],
            links: vec![TicketLink {
                kind: "figma".into(),
                url: "https://figma/x".into(),
                title: String::new(),
            }],
        }
    }

    /// The whole point of the rewrite: Jira Server stores wiki markup, and
    /// Markdown would land in the description as visible text.
    #[test]
    fn ticket_uses_wiki_markup_and_never_markdown() {
        let wiki = render_jira_ticket(&ticket()).wiki;
        assert!(!wiki.contains("##"), "markdown heading leaked: {wiki}");
        assert!(!wiki.contains("- [ ]"), "markdown checkbox leaked: {wiki}");
        assert!(!wiki.contains("|---|"), "markdown table rule leaked: {wiki}");
        assert!(wiki.starts_with("*1. Почему задача существует*\n"), "{wiki}");
        assert!(wiki.contains("\n - Кнопка «Оплачено» в карточке взноса"), "{wiki}");
    }

    /// Section names and the heading shape are copied from a real ticket in
    /// this tracker, so a generated description reads like a hand-written
    /// one rather than merely being valid markup.
    #[test]
    fn ticket_headings_match_the_tracker() {
        let wiki = render_jira_ticket(&ticket()).wiki;
        for heading in [
            "*1. Почему задача существует*",
            "*2. Что должно измениться после задачи*",
            "*3. Что входит в задачу*",
            "*4. Что не входит в задачу*",
            "*5. Техническое решение*",
            "*6. Критерии приемки* *- AC* *(функционал работает)*",
            "*7. Критерии выполнения - DoD (задача завершена правильно)*",
            "*8. Риски*",
            "*9. Ссылки*",
        ] {
            assert!(wiki.contains(heading), "missing {heading} in:\n{wiki}");
        }
    }

    /// A gap in the numbering ("…7, 9, 10") reads as a mistake, so numbers
    /// are assigned after the empty sections are dropped.
    #[test]
    fn ticket_numbers_sections_after_dropping_the_empty_ones() {
        let spec = JiraTicketSpec {
            why: "Проблема".into(),
            acceptance_criteria: vec!["Пользователь видит X".into()],
            ..Default::default()
        };
        let wiki = render_jira_ticket(&spec).wiki;
        assert!(wiki.starts_with("*1. Почему задача существует*"), "{wiki}");
        assert!(wiki.contains("*2. Критерии приемки*"), "{wiki}");
        assert!(!wiki.contains("Что должно измениться"), "empty section rendered: {wiki}");
        assert!(!wiki.contains("Риски"), "empty section rendered: {wiki}");
    }

    /// Grouped under a bold sub-heading, the way the tracker's own tickets
    /// carry them — not one flat list of URLs.
    #[test]
    fn ticket_groups_links_by_type_and_uppercases_it() {
        let spec = JiraTicketSpec {
            links: vec![
                TicketLink { kind: "git".into(), url: "https://git/a".into(), title: String::new() },
                TicketLink {
                    kind: "GIT".into(),
                    url: "https://git/b".into(),
                    title: "Спека".into(),
                },
                TicketLink { kind: String::new(), url: "https://bare".into(), title: String::new() },
            ],
            ..Default::default()
        };
        let wiki = render_jira_ticket(&spec).wiki;
        // Same type in two spellings is one group.
        assert_eq!(wiki.matches("*GIT*").count(), 1, "{wiki}");
        assert!(wiki.contains("*GIT*\nhttps://git/a\n[Спека|https://git/b]"), "{wiki}");
        // A link with no type has no sub-heading to file it under.
        assert!(wiki.contains("\n\nhttps://bare"), "{wiki}");
    }

    #[test]
    fn ticket_drops_blank_list_entries_and_rows() {
        let spec = JiraTicketSpec {
            in_scope: vec!["Реальный пункт".into(), "   ".into(), String::new()],
            links: vec![TicketLink { kind: "GIT".into(), url: "  ".into(), title: String::new() }],
            ..Default::default()
        };
        let wiki = render_jira_ticket(&spec).wiki;
        assert_eq!(wiki.matches("\n - ").count(), 1, "blank entries rendered: {wiki}");
        assert!(!wiki.contains("Ссылки"), "link without a URL rendered: {wiki}");
    }

    #[test]
    fn an_entirely_empty_ticket_renders_nothing_rather_than_bare_headings() {
        assert_eq!(render_jira_ticket(&JiraTicketSpec::default()).wiki, "");
    }
}
