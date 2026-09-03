//! Diagnostics service: computes broken references, duplicate anchors, missing
//! images, and circular includes over the `WorkspaceIndex` repositories.
//!
//! `run_all` recomputes diagnostics for every document; `run_for` recomputes
//! for a single document and its reverse-dependents (documents that include or
//! xref it). Results are written back into the index via `set_diagnostics`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::domain::settings::ErrorLanguage;
use crate::domain::workspace_index::{
    Diagnostic, DiagnosticKind, DocumentId, Severity,
};
use crate::infra::settings_store;
use crate::services::diagnostic_messages as msgs;
use crate::services::{openapi, openapi_lint};
use crate::services::workspace_index::WorkspaceIndex;

/// The `GeneralPrefs` entries that affect one diagnostics pass. Read once per
/// `run_all`/`run_for` call so a settings change applies on the next build.
/// A read error silently falls back to the defaults — having diagnostics at
/// all matters more than honouring the exact settings.
#[derive(Clone, Copy)]
struct DiagnosticsPrefs {
    lang: ErrorLanguage,
    /// Substitute the bundled copy of the common spec when the file is
    /// missing. While this is on, a `$ref` to that build artifact is served
    /// by the resolver (`openapi::Resolver::load_file`), so it is not a
    /// broken reference here either.
    openapi_ref_fallback: bool,
}

impl DiagnosticsPrefs {
    fn load() -> Self {
        let general = settings_store::load().map(|s| s.general).ok();
        Self {
            lang: general
                .as_ref()
                .map(|g| g.error_language)
                .unwrap_or_default(),
            openapi_ref_fallback: general
                .map(|g| g.openapi_ref_fallback_enabled)
                .unwrap_or(true),
        }
    }
}

/// Язык сообщений диагностик на этот проход. Читается из `GeneralPrefs` на
/// каждом вызове `run_all`/`run_for`, чтобы смена настройки сразу применялась
/// при следующем build. Ошибка чтения настроек тихо откатывается на `Ru`
/// (значение по умолчанию) — диагностики важнее, чем язык.
pub(crate) fn current_error_language() -> ErrorLanguage {
    settings_store::load()
        .map(|s| s.general.error_language)
        .unwrap_or_default()
}

/// Recompute diagnostics for every document in the index.
pub fn run_all(index: &WorkspaceIndex) {
    let prefs = DiagnosticsPrefs::load();
    // Спека собирается и проверяется один раз на весь проход: правила
    // считаются по собранному документу, а не по отдельному файлу.
    let spec = openapi_diagnostics(index, prefs);
    let docs = index.documents_iter();
    for d in &docs {
        let diags = diagnose_one_merged(index, &d.id, prefs, &spec);
        index.set_diagnostics(&d.id, diags);
    }
}

/// Recompute diagnostics for `doc` and every document that depends on it.
pub fn run_for(index: &WorkspaceIndex, doc: &DocumentId) {
    let prefs = DiagnosticsPrefs::load();

    // Файлы спецификации связаны не через include/xref, а через `$ref`, и в
    // сборке участвуют все сразу: правка одного фрагмента может убрать или
    // добавить находку в соседнем. Обход по зависимостям этого не выразит,
    // поэтому для спеки пересчитываем весь набор и раскладываем по её файлам.
    if is_spec_document(doc) {
        let spec = openapi_diagnostics(index, prefs);
        for d in &index.documents_iter() {
            if !is_spec_document(&d.id) {
                continue;
            }
            let diags = diagnose_one_merged(index, &d.id, prefs, &spec);
            index.set_diagnostics(&d.id, diags);
        }
        return;
    }

    let spec = HashMap::new();
    let mut queue: Vec<DocumentId> = vec![doc.clone()];
    let mut seen: HashSet<DocumentId> = HashSet::new();
    seen.insert(doc.clone());
    while let Some(current) = queue.pop() {
        let diags = diagnose_one_merged(index, &current, prefs, &spec);
        index.set_diagnostics(&current, diags);
        for dep in index.dependents_of(&current) {
            if seen.insert(dep.clone()) {
                queue.push(dep);
            }
        }
    }
}

/// Cross-document diagnostics plus the document-local entries already attached
/// to the document — `ParseError` (from the frontend asciidoctor logger) and
/// `DetachedHeaderAttributes`. Both are computed from one document's own text,
/// are orthogonal to include/xref/image checks, and must survive `run_all` at
/// the end of an async build.
fn diagnose_one_merged(
    index: &WorkspaceIndex,
    doc: &DocumentId,
    prefs: DiagnosticsPrefs,
    spec: &HashMap<DocumentId, Vec<Diagnostic>>,
) -> Vec<Diagnostic> {
    let mut diags = diagnose_one(index, doc, prefs);
    diags.extend(
        index
            .get_diagnostics_for(doc)
            .into_iter()
            .filter(|d| d.kind.is_document_local()),
    );
    if let Some(found) = spec.get(doc) {
        diags.extend(found.iter().cloned());
    }
    diags
}

fn diagnose_one(index: &WorkspaceIndex, doc: &DocumentId, prefs: DiagnosticsPrefs) -> Vec<Diagnostic> {
    let lang = prefs.lang;
    let mut out = Vec::new();

    // Missing include.
    for inc in index.find_includes(doc) {
        if index.document_exists_by_relative(&inc.path) {
            continue;
        }
        // A common spec missing from disk is not a broken reference while the
        // bundled-copy fallback is on: the bundler substitutes it.
        if prefs.openapi_ref_fallback && openapi::is_common_spec_fallback_path(Path::new(&inc.path))
        {
            continue;
        }
        out.push(Diagnostic {
            kind: DiagnosticKind::MissingInclude,
            message: msgs::missing_include(lang, &inc.path),
            document: doc.clone(),
            line: inc.line,
            column: inc.column,
            severity: Severity::Error,
        });
    }

    // Xref: missing document or missing anchor.
    for r in index.find_references(doc) {
        if r.target_document.is_empty() {
            // Pure `#anchor` reference within the same doc.
            if let Some(anchor) = &r.anchor {
                if !index.anchor_exists_in(doc, anchor) {
                    out.push(Diagnostic {
                        kind: DiagnosticKind::MissingXrefAnchor,
                        message: msgs::missing_xref_anchor_same_doc(lang, anchor),
                        document: doc.clone(),
                        line: r.line,
                        column: r.column,
                        severity: Severity::Error,
                    });
                }
            }
            continue;
        }
        if !index.document_exists_by_relative(&r.target_document) {
            out.push(Diagnostic {
                kind: DiagnosticKind::MissingXrefDocument,
                message: msgs::missing_xref_document(lang, &r.target_document),
                document: doc.clone(),
                line: r.line,
                column: r.column,
                severity: Severity::Error,
            });
            continue;
        }
        if let Some(anchor) = &r.anchor {
            let target_id = DocumentId::new(r.target_document.clone());
            if !index.anchor_exists_in(&target_id, anchor) {
                out.push(Diagnostic {
                    kind: DiagnosticKind::MissingXrefAnchor,
                    message: msgs::missing_xref_anchor(lang, &r.target_document, anchor),
                    document: doc.clone(),
                    line: r.line,
                    column: r.column,
                    severity: Severity::Error,
                });
            }
        }
    }

    // Missing image (path doesn't exist on disk under repo root).
    for img in index.images_for_doc(doc) {
        if !index.image_exists(&img.path) {
            out.push(Diagnostic {
                kind: DiagnosticKind::MissingImage,
                message: msgs::missing_image(lang, &img.path),
                document: doc.clone(),
                line: img.line,
                column: 1,
                severity: Severity::Error,
            });
        }
    }

    // Duplicate anchor (same id defined in >1 document).
    for a in index.find_anchors(doc) {
        if index.anchor_count(&a.id) > 1 {
            out.push(Diagnostic {
                kind: DiagnosticKind::DuplicateAnchor,
                message: msgs::duplicate_anchor(lang, &a.id),
                document: doc.clone(),
                line: a.line,
                column: a.column,
                severity: Severity::Warning,
            });
        }
    }

    // Циклический include (DFS от этого документа) — только для AsciiDoc.
    //
    // В `.adoc` цикл `include::` означает бесконечное раскрытие, то есть
    // настоящую ошибку вёрстки. В YAML/JSON рёбра графа строит `$ref`
    // (`infra::parsers::ref_utils`), а для спецификации цикл ссылок —
    // штатная конструкция: файлы-«бочки» ссылаются друг на друга, а
    // рекурсивные схемы (`Node.children: [Node]`) ссылаются сами на себя.
    // Сборщик разрешает это, вынося цель в `components/schemas`, и если
    // разрешить всё-таки не удалось — сообщает об этом сам
    // (`openapi::Resolver`). Ругаться здесь значит помечать исправный
    // спек-репозиторий десятками ошибок.
    if is_asciidoc_document(doc) {
        if let Some(cycle) = detect_cycle(index, doc) {
            out.push(Diagnostic {
                kind: DiagnosticKind::CircularInclude,
                message: msgs::circular_include(lang, &cycle.chain),
                document: doc.clone(),
                line: cycle.line,
                column: cycle.column,
                severity: Severity::Error,
            });
        }
    }

    out
}

fn is_asciidoc_document(doc: &DocumentId) -> bool {
    let path = doc.0.to_ascii_lowercase();
    path.ends_with(".adoc") || path.ends_with(".asciidoc")
}

/// Найденный цикл: сама цепочка для сообщения и позиция того `include::` в
/// исходном документе, с которого она начинается. Раньше диагностика всегда
/// вставала на 1:1, и перейти из панели «Проблемы» к виновной строке было
/// нельзя — при нескольких include в файле это заметная разница.
pub struct IncludeCycle {
    pub chain: Vec<String>,
    pub line: u32,
    pub column: u32,
}

/// DFS over the include graph from `start`, tracking the current chain.
/// Returns the chain as a `Vec<DocumentId>` if a cycle is found.
fn detect_cycle(index: &WorkspaceIndex, start: &DocumentId) -> Option<IncludeCycle> {
    fn dfs(
        index: &WorkspaceIndex,
        current: &DocumentId,
        chain: &mut Vec<DocumentId>,
        on_chain: &mut HashSet<DocumentId>,
    ) -> Option<Vec<DocumentId>> {
        if !on_chain.insert(current.clone()) {
            // Already on chain -> cycle. Trim to the cycle itself.
            let pos = chain.iter().position(|d| d == current).unwrap_or(0);
            return Some(chain[pos..].to_vec());
        }
        chain.push(current.clone());

        for inc in index.find_includes(current) {
            let target = DocumentId::new(inc.path.clone());
            if index.document_exists_by_relative(&inc.path) {
                if let Some(cycle) = dfs(index, &target, chain, on_chain) {
                    return Some(cycle);
                }
            }
        }

        chain.pop();
        on_chain.remove(current);
        None
    }

    let mut chain = Vec::new();
    let mut on_chain = HashSet::new();
    let cycle = dfs(index, start, &mut chain, &mut on_chain)?;

    // Позиция берётся у первого include в самом документе, ведущего в цикл:
    // именно его правка цикл и разрывает.
    let next = cycle.get(1).or_else(|| cycle.first());
    let (line, column) = next
        .and_then(|target| {
            index
                .find_includes(start)
                .into_iter()
                .find(|inc| inc.path == target.0)
                .map(|inc| (inc.line, inc.column))
        })
        .unwrap_or((1, 1));

    Some(IncludeCycle {
        chain: cycle
            .into_iter()
            .map(|d| d.0.clone())
            .chain(std::iter::once(start.0.clone()))
            .collect(),
        line,
        column,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::parsers::registry::ParserRegistry;
    use crate::services::workspace_index::WorkspaceIndex;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Nanosecond timestamps alone aren't guaranteed unique across threads
    // (clock resolution varies by platform), and tests run in parallel by
    // default, so a collision would make two tests share a directory and
    // clobber each other's same-named fixture files. Mix in a process-wide
    // counter to guarantee uniqueness regardless of clock resolution.
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-diag-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Spec repo с двумя настоящими находками: у операции нет 4xx, а тело
    /// запроса объявлено без схемы. Обе живут во фрагменте, а не во входном
    /// документе, — как в реальном многофайловом репозитории.
    fn repo_with_lint_findings() -> PathBuf {
        let root = temp_dir();
        fs::create_dir_all(root.join("specs").join("operations")).unwrap();
        fs::create_dir_all(root.join("specs").join("schemas")).unwrap();
        fs::write(
            root.join("specs/api.yaml"),
            concat!(
                "openapi: 3.0.3\n",
                "info:\n  title: t\n  version: '1'\n",
                "servers:\n  - url: https://api.example.com\n",
                "paths:\n",
                "  /pets:\n",
                "    post:\n      $ref: './operations/createPet.yaml'\n",
            ),
        )
        .unwrap();
        fs::write(
            root.join("specs/operations/createPet.yaml"),
            concat!(
                "tags:\n  - pets\n",
                "summary: Создать питомца\n",
                "operationId: createPet\n",
                "requestBody:\n",
                "  content:\n",
                "    application/json: {}\n",
                "responses:\n",
                "  '201':\n",
                "    description: Создан\n",
            ),
        )
        .unwrap();
        root
    }

    #[test]
    fn openapi_findings_point_at_the_real_line_in_the_source_file() {
        let root = repo_with_lint_findings();
        let index = build(&root);
        run_all(&index);

        let doc = DocumentId("specs/operations/createPet.yaml".to_string());
        let diagnostics: Vec<Diagnostic> = index
            .get_diagnostics_for(&doc)
            .into_iter()
            .filter(|d| d.kind == DiagnosticKind::OpenapiRule)
            .collect();

        assert!(
            !diagnostics.is_empty(),
            "ожидались находки правил, получили: {:?}",
            index.get_diagnostics_for(&doc)
        );
        assert!(
            diagnostics.iter().all(|d| d.line > 1),
            "находки должны указывать на строку операции, а не на начало файла: {:?}",
            diagnostics
                .iter()
                .map(|d| (d.line, d.message.clone()))
                .collect::<Vec<_>>()
        );

        fs::remove_dir_all(&root).ok();
    }

    fn build(root: &std::path::Path) -> Arc<WorkspaceIndex> {
        let idx = Arc::new(WorkspaceIndex::new(ParserRegistry::new()));
        idx.build(root.to_path_buf()).unwrap();
        idx
    }

    /// Spec repo whose `specs/responses/all.yaml` `$ref`s the well-known
    /// common-spec build artifact, without that artifact existing on disk —
    /// a spec repo opened without a Gradle build having run.
    fn repo_with_missing_common_spec_ref() -> PathBuf {
        let root = temp_dir();
        fs::create_dir_all(root.join("specs").join("responses")).unwrap();
        fs::write(
            root.join("specs").join("responses").join("all.yaml"),
            "BadRequest:\n  $ref: ../../build/common/META-INF/specs/api.yaml#/components/responses/BadRequest\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn missing_common_spec_ref_is_not_reported_when_fallback_enabled() {
        let root = repo_with_missing_common_spec_ref();
        let diags = settings_store::test_support::with_temp_home(|| {
            // No settings file written: `openapi_ref_fallback_enabled`
            // defaults to on.
            build(&root).get_diagnostics()
        });
        assert!(
            diags
                .iter()
                .all(|d| d.kind != DiagnosticKind::MissingInclude),
            "expected no missing-include diagnostic, got {diags:?}"
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_common_spec_ref_is_reported_when_fallback_disabled() {
        let root = repo_with_missing_common_spec_ref();
        let diags = settings_store::test_support::with_temp_home(|| {
            let mut settings = crate::domain::settings::AppSettings::default();
            settings.general.openapi_ref_fallback_enabled = false;
            settings_store::save(&settings).unwrap();
            build(&root).get_diagnostics()
        });
        assert!(
            diags
                .iter()
                .any(|d| d.kind == DiagnosticKind::MissingInclude),
            "expected a missing-include diagnostic, got {diags:?}"
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_missing_include() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "include::missing.adoc[]\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::MissingInclude));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_circular_include() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "include::b.adoc[]\n").unwrap();
        fs::write(root.join("b.adoc"), "include::a.adoc[]\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::CircularInclude));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn circular_include_points_at_the_line_that_closes_the_cycle() {
        let root = temp_dir();
        fs::write(
            root.join("a.adoc"),
            "= Заголовок\n\nТекст\n\ninclude::b.adoc[]\n",
        )
        .unwrap();
        fs::write(root.join("b.adoc"), "include::a.adoc[]\n").unwrap();
        let idx = build(&root);
        let cycle = idx
            .get_diagnostics()
            .into_iter()
            .find(|d| d.kind == DiagnosticKind::CircularInclude && d.document.0 == "a.adoc")
            .expect("цикл должен быть найден");
        assert_eq!(cycle.line, 5, "диагностика должна вести к самому include");
        fs::remove_dir_all(&root).ok();
    }

    /// Файлы спецификации ссылаются друг на друга через `$ref`, и цикл среди
    /// них — норма: и «бочки» вроде `all.yaml`, и рекурсивные схемы. Сборщик
    /// с этим справляется, поэтому правило про циклический include (писанное
    /// для `include::` в AsciiDoc) на них распространяться не должно —
    /// иначе исправный спек-репозиторий помечается десятками ошибок.
    #[test]
    fn a_ref_cycle_between_spec_files_is_not_a_circular_include() {
        let root = temp_dir();
        fs::create_dir_all(root.join("specs").join("schemas")).unwrap();
        fs::write(
            root.join("specs/api.yaml"),
            "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\n",
        )
        .unwrap();
        fs::write(
            root.join("specs/schemas/all.yaml"),
            "Pet:\n  $ref: './common.yaml#/Pet'\n",
        )
        .unwrap();
        fs::write(
            root.join("specs/schemas/common.yaml"),
            "Pet:\n  $ref: './all.yaml#/Pet'\n",
        )
        .unwrap();

        let idx = build(&root);
        let cycles: Vec<Diagnostic> = idx
            .get_diagnostics()
            .into_iter()
            .filter(|d| d.kind == DiagnosticKind::CircularInclude)
            .collect();
        assert!(
            cycles.is_empty(),
            "цикл $ref между файлами спеки — не ошибка, получили {:?}",
            cycles.iter().map(|d| d.message.clone()).collect::<Vec<_>>()
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_self_include() {
        let root = temp_dir();
        fs::write(root.join("self.adoc"), "include::self.adoc[]\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::CircularInclude));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_duplicate_anchor() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "[[dup]]\n= A\n").unwrap();
        fs::write(root.join("b.adoc"), "[[dup]]\n= B\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::DuplicateAnchor));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_missing_xref_document() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "xref:nope.adoc[]\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags
            .iter()
            .any(|d| d.kind == DiagnosticKind::MissingXrefDocument));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_missing_xref_anchor() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "xref:b.adoc#missing[]\n").unwrap();
        fs::write(root.join("b.adoc"), "= B\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags
            .iter()
            .any(|d| d.kind == DiagnosticKind::MissingXrefAnchor));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detects_missing_image() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "image::nope.png[]\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.iter().any(|d| d.kind == DiagnosticKind::MissingImage));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn clean_doc_has_no_diagnostics() {
        let root = temp_dir();
        fs::write(root.join("a.adoc"), "[[ok]]\n= A\n").unwrap();
        let idx = build(&root);
        let diags = idx.get_diagnostics();
        assert!(diags.is_empty(), "got: {:?}", diags);
        fs::remove_dir_all(&root).ok();
    }
}
/// Файл спецификации: под `specs/` и в формате, который умеет читать сборщик.
fn is_spec_document(doc: &DocumentId) -> bool {
    let path = doc.0.replace('\\', "/");
    if !path.starts_with("specs/") {
        return false;
    }
    matches!(
        path.rsplit('.').next(),
        Some("yaml") | Some("yml") | Some("json")
    )
}

/// Ищет запись карты источников, покрывающую `pointer`: точное совпадение
/// выигрывает у предка, предок — у корневой записи. Записи должны быть
/// отсортированы от самого длинного указателя к самому короткому.
fn source_for_pointer<'a>(
    sources: &'a [crate::domain::openapi::SourceRef],
    pointer: &str,
) -> Option<&'a crate::domain::openapi::SourceRef> {
    sources.iter().find(|entry| {
        entry.pointer.is_empty()
            || pointer == entry.pointer
            || pointer.starts_with(&format!("{}/", entry.pointer))
    })
}

/// Проблемы спецификации, разложенные по файлам-исходникам.
///
/// Считаются по собранному документу, поэтому проход один на весь `specs/`, а
/// не на каждый файл. Адрес находки внутри сборки сам по себе бесполезен —
/// в многофайловой спеке нужно назвать конкретный файл, — поэтому её положение
/// восстанавливается через карту источников (`SourceRef`), а строка внутри
/// файла ищется текстом (`openapi_lint::find_spec_line`).
fn openapi_diagnostics(
    index: &WorkspaceIndex,
    prefs: DiagnosticsPrefs,
) -> HashMap<DocumentId, Vec<Diagnostic>> {
    let mut out: HashMap<DocumentId, Vec<Diagnostic>> = HashMap::new();

    let Some(repo_root) = index.repo_root() else {
        return out;
    };
    let Ok(Some(info)) = openapi::detect_specs_repo(&repo_root) else {
        return out;
    };
    let Ok(bundle) = openapi::load_openapi_bundle(
        &repo_root,
        &info.entry_file,
        prefs.openapi_ref_fallback,
    ) else {
        return out;
    };

    let mut sources = bundle.sources.clone();
    sources.sort_by(|a, b| b.pointer.len().cmp(&a.pointer.len()));

    let mut texts: HashMap<String, String> = HashMap::new();
    let mut line_of = |file: &str, keys: &[String]| -> u32 {
        let text = texts.entry(file.to_string()).or_insert_with(|| {
            std::fs::read_to_string(repo_root.join(file)).unwrap_or_default()
        });
        if text.is_empty() {
            1
        } else {
            openapi_lint::find_spec_line(text, keys)
        }
    };

    // Неразрешённые `$ref` резолвер уже назвал вместе с файлом-источником.
    for diagnostic in &bundle.diagnostics {
        let file = diagnostic.referenced_from.clone();
        let line = line_of(&file, &[diagnostic.reference.clone()]);
        out.entry(DocumentId(file.clone()))
            .or_default()
            .push(Diagnostic {
                kind: DiagnosticKind::OpenapiRef,
                message: msgs::oas_unresolved_ref(
                    prefs.lang,
                    &diagnostic.reference,
                    &diagnostic.reason,
                ),
                document: DocumentId(file),
                line,
                column: 1,
                severity: Severity::Error,
            });
    }

    for finding in openapi_lint::lint(&bundle.document, prefs.lang) {
        let Some(source) = source_for_pointer(&sources, &finding.pointer) else {
            continue;
        };
        let mut keys: Vec<String> = Vec::new();
        if let Some(last) = source.fragment.rsplit('/').find(|s| !s.is_empty()) {
            keys.push(last.replace("~1", "/").replace("~0", "~"));
        }
        if let Some(operation) = &finding.operation {
            let pointer = openapi_lint::operation_pointer(&operation.path, &operation.method);
            if let Some(id) = bundle
                .document
                .pointer(&pointer)
                .and_then(|op| op.get("operationId"))
                .and_then(|v| v.as_str())
            {
                keys.push(id.to_string());
            }
            keys.push(operation.path.clone());
        }

        let file = source.file.clone();
        let line = line_of(&file, &keys);
        let message = match &finding.operation {
            Some(operation) => format!(
                "{}{}",
                msgs::oas_operation_prefix(&operation.method.to_uppercase(), &operation.path),
                finding.message
            ),
            None => finding.message.clone(),
        };
        out.entry(DocumentId(file.clone()))
            .or_default()
            .push(Diagnostic {
                kind: DiagnosticKind::OpenapiRule,
                message,
                document: DocumentId(file),
                line,
                column: 1,
                severity: finding.severity,
            });
    }

    for diagnostics in out.values_mut() {
        diagnostics.sort_by_key(|d| d.line);
    }
    out
}

