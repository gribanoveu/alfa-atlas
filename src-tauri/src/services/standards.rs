//! Orchestrates the API-documentation standards checker: walks the docs
//! root, groups files into `methodName` folders (per К.1.1 of the standard),
//! runs every enabled rule from `standards_rules::RULES` against each, and
//! scores the result.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::domain::standards::{Finding, FolderReport, StandardsReport, StandardsRuleConfig};
use crate::domain::workspace_index::unix_seconds;
use crate::services::standards_rules::{FileEntry, MethodFolderCtx, RULES};

/// Directory names excluded from the check, per the standard's "Исключения"
/// table (§6 for К.1.1). Compared case-insensitively, so listing both cases
/// from the standard is redundant but harmless.
const EXCLUDED_DIR_NAMES: &[&str] = &["external", "_external", "config", "template"];

/// Directory name masks excluded from the check (case-insensitive).
fn matches_excluded_dir_mask(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with("external_") || lower.starts_with("lib_") || lower.contains("soap")
}

/// Individual files excluded from role-matching / content checks, but that
/// do not disqualify the folder they live in.
const EXCLUDED_FILE_NAMES: &[&str] = &[
    "index.adoc",
    "db.adoc",
    "config.adoc",
    "compositeexception.adoc",
    "systemparams.adoc",
    "_common.adoc",
    "_configs.adoc",
    "kafka.adoc",
    "mongodb.adoc",
];

/// Text file extensions relevant to the checker's rules; everything else in
/// a method folder (images, binaries) is ignored.
const RELEVANT_EXTENSIONS: &[&str] = &[".adoc", ".asciidoc", ".puml", ".plantuml"];

/// Marker text (§6 of the standard) that excludes a whole file/folder from
/// the check. Matched case-insensitively as a substring.
const EXCEPTION_WHOLE_FILE: &str = "exceptions - файл исключен из проверки документации";
const EXCEPTION_INPUT_PARAMS: &str =
    "exceptions - описание входных параметров исключено из проверки документации";
const EXCEPTION_OUTPUT_PARAMS: &str =
    "exceptions - описание выходных параметров исключено из проверки документации";

fn is_excluded_dir_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    EXCLUDED_DIR_NAMES.contains(&lower.as_str())
}

fn is_excluded_file_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    EXCLUDED_FILE_NAMES.contains(&lower.as_str())
}

fn is_relevant_extension(name: &str) -> bool {
    let lower = name.to_lowercase();
    RELEVANT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(needle)
}

/// Run every enabled rule against the docs root, grouped by `methodName`
/// folder. Returns an empty report (with `overall_passed = false`) if the
/// docs root doesn't exist or contains no recognizable method folder.
pub fn check_repository(docs_root: &Path, config: &StandardsRuleConfig) -> StandardsReport {
    let mut folders = Vec::new();
    if docs_root.is_dir() {
        walk(docs_root, docs_root, config, &mut folders);
    }
    folders.sort_by(|a, b| a.folder.cmp(&b.folder));
    let overall_passed = !folders.is_empty() && folders.iter().all(|f| f.passed);
    StandardsReport {
        folders,
        overall_passed,
        checked_at: unix_seconds(SystemTime::now()),
    }
}

fn walk(
    docs_root: &Path,
    dir: &Path,
    config: &StandardsRuleConfig,
    out: &mut Vec<FolderReport>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut subdirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || is_excluded_dir_name(&name) || matches_excluded_dir_mask(&name) {
                continue;
            }
            subdirs.push(path);
        } else if file_type.is_file() {
            files.push(path);
        }
    }

    let has_own_adoc = files.iter().any(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        name.to_lowercase().ends_with(".adoc") && !is_excluded_file_name(&name)
    });

    // The docs root itself is a container, not a method folder, even if it
    // has loose top-level files (e.g. a repo-wide index.adoc).
    if has_own_adoc && dir != docs_root {
        if let Some(report) = evaluate_method_folder(docs_root, dir, &files, config) {
            out.push(report);
        }
    }

    for sub in subdirs {
        walk(docs_root, &sub, config, out);
    }
}

fn evaluate_method_folder(
    docs_root: &Path,
    dir: &Path,
    files: &[PathBuf],
    config: &StandardsRuleConfig,
) -> Option<FolderReport> {
    let method_name = dir.file_name()?.to_string_lossy().into_owned();

    let mut entries = Vec::new();
    for path in files {
        let name = path.file_name()?.to_string_lossy().into_owned();
        if is_excluded_file_name(&name) || !is_relevant_extension(&name) {
            continue;
        }
        let content = fs::read_to_string(path).unwrap_or_default();
        entries.push(FileEntry { name, path: path.clone(), content });
    }

    if entries.iter().any(|f| contains_ci(&f.content, EXCEPTION_WHOLE_FILE)) {
        return None;
    }

    let has_input_exception = entries.iter().any(|f| contains_ci(&f.content, EXCEPTION_INPUT_PARAMS));
    let has_output_exception = entries.iter().any(|f| contains_ci(&f.content, EXCEPTION_OUTPUT_PARAMS));

    let ctx = MethodFolderCtx::new(method_name.clone(), entries);

    let mut findings = Vec::new();
    let mut score = 0u32;
    let mut max_score = 0u32;

    for rule in RULES {
        if !config.is_enabled(&rule.def) {
            continue;
        }
        max_score += rule.def.weight;

        let auto_pass = (rule.def.id.starts_with("K.4") && has_input_exception)
            || (rule.def.id.starts_with("K.5") && has_output_exception);

        let outcome = if auto_pass {
            crate::domain::standards::RuleOutcome {
                passed: true,
                message: crate::services::standards_messages::passed(current_error_language()),
            }
        } else {
            (rule.check)(&ctx)
        };

        if outcome.passed {
            score += rule.def.weight;
        }
        findings.push(Finding {
            rule_id: rule.def.id.to_string(),
            title: rule.def.title.to_string(),
            passed: outcome.passed,
            weight: rule.def.weight,
            message: outcome.message,
        });
    }

    let passed = max_score > 0 && score * 100 > max_score * 80;
    let folder = crate::domain::paths::relative_to(docs_root, dir).unwrap_or(method_name.clone());

    Some(FolderReport {
        folder,
        method_name,
        score,
        max_score,
        passed,
        findings,
    })
}

fn current_error_language() -> crate::domain::settings::ErrorLanguage {
    crate::infra::settings_store::load()
        .map(|s| s.general.error_language)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use super::*;
    use std::time::{SystemTime as ST, UNIX_EPOCH};

    /// Several tests in this module call this concurrently. A nanosecond
    /// timestamp alone does not reliably disambiguate them on a coarser
    /// system clock — two would share a directory and clobber each other.
    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = ST::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-standards-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_full_method(root: &Path, method: &str) {
        let dir = root.join(method);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{method}.puml")), "@startuml\n@enduml").unwrap();
        let main = format!(
            "= {method}\n:toc:\n\n== Назначение\nЭто достаточно длинное описание метода для прохождения проверки на пятьдесят символов.\n\n== Схема работы\nimage::{method}.puml[]\n\n== Описание входных параметров\n|===\n| Имя | Тип | Обязательный | Описание\n| id | string | да | идентификатор\n|===\n\ninclude::./request.adoc[]\n\n== Описание выходных параметров\n|===\n| Имя | Тип | Обязательный | Описание\n| id | string | да | идентификатор\n|===\n\ninclude::./response.adoc[]\n\n== Алгоритм работы\nШаг 1.\n\n== Обработка ошибок\n404 - не найдено.\n"
        );
        fs::write(dir.join(format!("{method}.adoc")), main).unwrap();
        fs::write(dir.join("request.adoc"), "${HOST}/api/x\ncurl example").unwrap();
        fs::write(dir.join("response.adoc"), "{}").unwrap();
    }

    /// Thrift-style method folder with vertical input tables and multi-line /
    /// rowspan output tables — the layout that previously tripped K.4.2/K.5.2.
    fn write_thrift_method_with_multiline_tables(root: &Path) {
        const METHOD: &str = "fetchAusnTransactions";
        let dir = root.join(METHOD);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join(format!("{METHOD}.puml")), "@startuml\n@enduml").unwrap();

        fs::write(
            dir.join(format!("{METHOD}.adoc")),
            r#"= fetchAusnTransactions
:toc:

== Назначение метода

Данный метод предназначен для получения транзакций Клиента на режиме АУСН.

== Описание входных/выходных параметров

=== Входные параметры

include::request.adoc[]

=== Выходные параметры

include::response.adoc[]

== Схема работы

include::./fetchAusnTransactions.puml[]

== Алгоритм работы

1. Валидация входных параметров.

== Обработка ошибок

|===
|*Условие* |*Описание* |*Type* |*Code*

| Не прошла валидация userData
| Входные параметры не прошли валидацию
| ValidationException
| validationError
|="#,
        )
        .unwrap();

        fs::write(
            dir.join("request.adoc"),
            r#"[cols="4"]
|===
|*Параметр* |*Тип данных* |*Обязательность* |*Описание*
|userData.id
|string
|required
|Мнемоника пользователя
|transactionFilter.organizationId
|string
|required
|Идентификатор организации
|===

.Endpoint:${HOST}/wlbuh-ausn-api/tapi
{"organizationId":"UBV1IL"}
"#,
        )
        .unwrap();

        fs::write(
            dir.join("response.adoc"),
            r#"[cols="4"]
|===
|*Параметр* |*Тип данных* |*Обязательность* |*Описание*
|lastRequestDateTime|UNIX datetime|required|Дата и время последнего запроса
|transactions|object[]|required|Список транзакций
|===

[cols="4"]
|===
|*Параметр* |*Тип данных* |*Обязательность* |*Описание*

|
1: transaction[].id +
2: transaction[].organizationId
|
1: string +
2: string
|
1: required +
2: required
|
1: Ключ полупроводки. +
2: CUS Клиента
|===

|===
3+| *Поле* | *Тип* | *Обязательность* |*Описание*
.15+|specifics .5+|single[] | OperationTaxbaseCode | String | required | Разметка 1
|OperationCategory |string |optional|Разметка 2
|===

[cols="5"]
|===
2+|*Поле* |*Тип данных*|*Обязательность* |*Описание*
.4+|page|number|i32|required |Номер страницы
|size|i32|required|Кол-во элементов на странице
|===

{"lastRequestDateTime":null}
"#,
        )
        .unwrap();
    }

    #[test]
    fn full_method_folder_passes() {
        let root = temp_dir();
        write_full_method(&root, "getUser");
        let config = StandardsRuleConfig::default();
        let report = check_repository(&root, &config);
        assert_eq!(report.folders.len(), 1);
        assert!(report.folders[0].passed, "{:?}", report.folders[0]);
        assert!(report.overall_passed);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_files_fail_the_folder() {
        let root = temp_dir();
        let dir = root.join("getUser");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("getUser.adoc"), "= getUser").unwrap();
        let config = StandardsRuleConfig::default();
        let report = check_repository(&root, &config);
        assert_eq!(report.folders.len(), 1);
        assert!(!report.folders[0].passed);
        assert!(!report.overall_passed);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn excluded_folder_name_is_skipped() {
        let root = temp_dir();
        write_full_method(&root.join("external"), "getUser");
        let config = StandardsRuleConfig::default();
        let report = check_repository(&root, &config);
        assert!(report.folders.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn excluded_folder_mask_is_skipped() {
        let root = temp_dir();
        write_full_method(&root.join("lib_shared"), "getUser");
        let config = StandardsRuleConfig::default();
        let report = check_repository(&root, &config);
        assert!(report.folders.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn whole_file_exception_marker_excludes_folder() {
        let root = temp_dir();
        let dir = root.join("legacy");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("legacy.adoc"),
            "= legacy\nException notice below.\n\nExceptions - Файл исключен из проверки документации",
        )
        .unwrap();
        let config = StandardsRuleConfig::default();
        let report = check_repository(&root, &config);
        assert!(report.folders.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    /// The folder `createDirectory` scaffolds is the state every method
    /// documentation starts from, and the skill tells the model to run `check`
    /// on what it wrote — so the untouched scaffold has to pass the checker on
    /// its own, before a single placeholder is replaced.
    #[test]
    fn the_rest_endpoint_scaffold_passes_the_standards_checker() {
        let root = temp_dir();
        crate::services::docs_fs::create_rest_endpoint_folder(
            root.to_str().unwrap(),
            "getUserProfile",
            "getUserProfile",
        )
        .unwrap();
        // `../CompositeException.adoc` lives one level above the method
        // folder, exactly as the standard's include expects.
        fs::write(
            root.join("CompositeException.adoc"),
            "Общее описание формата ошибки.\n",
        )
        .unwrap();

        let report = check_repository(&root, &StandardsRuleConfig::default());
        let folder = report
            .folders
            .iter()
            .find(|f| f.method_name == "getUserProfile")
            .expect("scaffolded folder is discovered");
        let failed: Vec<&str> = folder
            .findings
            .iter()
            .filter(|f| !f.passed)
            .map(|f| f.rule_id.as_str())
            .collect();
        assert!(
            failed.is_empty(),
            "scaffold fails {failed:?} ({}/{})",
            folder.score,
            folder.max_score
        );
        assert!(folder.passed);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn input_exception_marker_autopasses_k4_rules() {
        let root = temp_dir();
        let dir = root.join("getUser");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("getUser.puml"), "@startuml\n@enduml").unwrap();
        fs::write(
            dir.join("getUser.adoc"),
            "= getUser\n:toc:\n\n== Назначение\nЭто достаточно длинное описание метода для прохождения проверки на пятьдесят символов.\n\n== Схема работы\nimage::getUser.puml[]\n\n== Описание входных параметров\nExceptions - Описание входных параметров исключено из проверки документации\n\n== Описание выходных параметров\n|===\n| Имя | Тип | Обязательный | Описание\n| id | string | да | идентификатор\n|===\n\ninclude::./response.adoc[]\n\n== Алгоритм работы\nШаг 1.\n\n== Обработка ошибок\n404.\n",
        )
        .unwrap();
        fs::write(dir.join("request.adoc"), "").unwrap();
        fs::write(dir.join("response.adoc"), "{}").unwrap();

        let config = StandardsRuleConfig::default();
        let report = check_repository(&root, &config);
        assert_eq!(report.folders.len(), 1);
        let folder = &report.folders[0];
        for id in ["K.4.1", "K.4.2", "K.4.3", "K.4.4"] {
            let finding = folder.findings.iter().find(|f| f.rule_id == id).unwrap();
            assert!(finding.passed, "{id} should auto-pass");
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn disabling_a_rule_reduces_max_score() {
        let root = temp_dir();
        write_full_method(&root, "getUser");
        let mut config = StandardsRuleConfig::default();
        config.rules.insert("K.7.1".to_string(), false);
        let report = check_repository(&root, &config);
        assert_eq!(report.folders[0].max_score, 94);
        assert!(report.folders[0].findings.iter().all(|f| f.rule_id != "K.7.1"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn empty_docs_root_does_not_pass() {
        let root = temp_dir();
        let config = StandardsRuleConfig::default();
        let report = check_repository(&root, &config);
        assert!(report.folders.is_empty());
        assert!(!report.overall_passed);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn thrift_method_with_multiline_tables_passes_standards_check() {
        let root = temp_dir();
        write_thrift_method_with_multiline_tables(&root);
        let config = StandardsRuleConfig::default();
        let report = check_repository(&root, &config);
        assert_eq!(report.folders.len(), 1);
        let folder = &report.folders[0];
        let failures: Vec<_> = folder
            .findings
            .iter()
            .filter(|f| !f.passed)
            .map(|f| format!("{}: {}", f.rule_id, f.message))
            .collect();
        assert!(
            folder.passed,
            "score {}/{}; failures: {:?}",
            folder.score,
            folder.max_score,
            failures
        );
        fs::remove_dir_all(&root).ok();
    }
}
