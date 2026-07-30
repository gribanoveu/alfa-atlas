use serde::{Deserialize, Serialize};

const METHOD_TEMPLATE: &str =
    include_str!("../../../src/templates/asciidoc/rest-endpoint/methodName.adoc");
const REQUEST_TEMPLATE: &str =
    include_str!("../../../src/templates/asciidoc/rest-endpoint/request.adoc");
const RESPONSE_TEMPLATE: &str =
    include_str!("../../../src/templates/asciidoc/rest-endpoint/response.adoc");
pub const SEQUENCE_DIAGRAM_TEMPLATE: &str =
    include_str!("../../../src/templates/asciidoc/rest-endpoint/sequence_diagramm.puml");

/// Selectable AsciiDoc templates for a single new file. The sequence-diagram
/// template isn't offered standalone here — it only ships as part of the
/// REST-endpoint folder template (see `SEQUENCE_DIAGRAM_TEMPLATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AsciidocFileTemplate {
    Method,
    Request,
    Response,
}

impl AsciidocFileTemplate {
    pub fn content(self) -> &'static str {
        match self {
            AsciidocFileTemplate::Method => METHOD_TEMPLATE,
            AsciidocFileTemplate::Request => REQUEST_TEMPLATE,
            AsciidocFileTemplate::Response => RESPONSE_TEMPLATE,
        }
    }
}
