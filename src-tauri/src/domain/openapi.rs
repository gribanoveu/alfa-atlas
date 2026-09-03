use serde::Serialize;
use thiserror::Error;

use super::project_config::ProjectError;

/// Metadata about a detected OpenAPI spec repository (a `specs/` folder at
/// the repo root containing `schemas/`, `responses/`, `parameters/`,
/// `operations/`, plus an entry document with a top-level `openapi:`/`swagger:` key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecsRepoInfo {
    /// Absolute, canonicalized path to `{repoRoot}/specs`.
    pub specs_root: String,
    /// Path to the entry document, relative to the repo root.
    pub entry_file: String,
    pub title: Option<String>,
    pub version: Option<String>,
}

/// A `$ref` that could not be fully resolved while bundling, recorded instead
/// of failing the whole load. `pointer` locates the offending node within the
/// *output* bundled document (a JSON Pointer), so the frontend can find it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefDiagnostic {
    pub pointer: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub referenced_from: String,
    pub reason: String,
}

/// Откуда в собранный документ попал узел по адресу `pointer`. Пишется на
/// каждой границе `$ref`, поэтому источник произвольного узла — это запись с
/// самым длинным `pointer`-префиксом от него (узлы, объявленные прямо во
/// входном документе, попадают под корневую запись).
///
/// Нужен и вьюеру («открыть исходник операции» — в многофайловой спеке иначе
/// не найти, в каком из сотни файлов лежит ручка), и правилам валидации,
/// которым надо назвать конкретный файл, а не адрес внутри сборки.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    /// JSON Pointer в собранном документе.
    pub pointer: String,
    /// Файл-источник относительно корня репозитория.
    pub file: String,
    /// JSON Pointer внутри файла-источника; пустой — ссылка на файл целиком.
    pub fragment: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiBundleResult {
    pub document: serde_json::Value,
    pub diagnostics: Vec<RefDiagnostic>,
    pub sources: Vec<SourceRef>,
}

#[derive(Debug, Error)]
pub enum OpenApiError {
    #[error(transparent)]
    Path(#[from] ProjectError),
    #[error("failed to read {0}: {1}")]
    Read(String, #[source] std::io::Error),
    #[error("failed to parse {0}: {1}")]
    Parse(String, String),
    #[error("entry file has no top-level `openapi`/`swagger` key: {0}")]
    NotOpenApi(String),
}
