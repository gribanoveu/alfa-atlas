//! `getAsciidocTemplates` — a lookup against the fixed element catalog.
//!
//! The only tool here that cannot fail: an unmatched id comes back in
//! `not_found` rather than as a `ToolError`, so a partly-wrong batch still
//! returns the templates it did resolve.

use crate::domain::ai_tools::{AsciidocTemplateEntry, GetAsciidocTemplatesArgs, ToolResult};
use crate::domain::asciidoc_element_templates::{
    ASCIIDOC_ELEMENT_TEMPLATES, find_many as find_asciidoc_templates,
};
use crate::domain::llm::LlmToolDefinition;

/// Executes `getAsciidocTemplates` — a pure in-memory lookup against the
/// fixed `domain::asciidoc_element_templates::ASCIIDOC_ELEMENT_TEMPLATES`
/// catalog, so unlike almost every other tool here this cannot fail: an
/// empty or entirely-unmatched `ids` just yields an empty/`not_found`-only
/// result rather than a `ToolError`.
pub(super) fn get_asciidoc_templates(args: GetAsciidocTemplatesArgs) -> ToolResult {
    let (found, not_found) = find_asciidoc_templates(&args.ids);
    let templates = found
        .into_iter()
        .map(|t| AsciidocTemplateEntry {
            id: t.id.to_string(),
            label: t.label.to_string(),
            category: t.category.to_string(),
            template: t.template.to_string(),
        })
        .collect();
    ToolResult::AsciidocTemplates { templates, not_found }
}

/// Renders `ASCIIDOC_ELEMENT_TEMPLATES` as a compact, grouped index for
/// `getAsciidocTemplates`'s own tool description — generated straight from
/// the same catalog `get_asciidoc_templates` looks ids up in, so the index
/// the model sees can never list an id the tool can't actually resolve (or
/// vice versa). Relies on the catalog already being grouped by category in
/// declaration order (structure, tables, examples, includes).
pub(super) fn asciidoc_template_catalog_description() -> String {
    fn category_label(category: &str) -> &str {
        match category {
            "structure" => "Структура",
            "tables" => "Таблицы",
            "examples" => "Примеры",
            "includes" => "Вставки",
            other => other,
        }
    }
    let mut out = String::new();
    let mut current_category = "";
    for t in ASCIIDOC_ELEMENT_TEMPLATES {
        if t.category != current_category {
            out.push_str(category_label(t.category));
            out.push_str(":\n");
            current_category = t.category;
        }
        out.push_str(&format!("- `{}`: {} — {}\n", t.id, t.label, t.description));
    }
    out
}

/// The `getAsciidocTemplates` schema the model sees.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "getAsciidocTemplates".to_string(),
        description: format!(
            "Fetch the full canonical AsciiDoc markup for one or more house element templates (tables, admonitions, lists, includes, etc.) by id, from this fixed catalog:\n\n{}\nCall this before drafting a table, admonition block, list, or include that matches one of the entries above, passing its `id` (multiple ids may be requested in one call). Reuse the returned markup as the baseline for what you write — only placeholder values/content change — instead of inventing different syntax. If none of the entries fit the specific need, plain AsciiDoc without calling this is fine.",
            asciidoc_template_catalog_description()
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "description": "One or more template ids from the catalog above."
                }
            },
            "required": ["ids"]
        }),
        }
}
