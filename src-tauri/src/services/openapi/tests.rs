use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::bundle::*;
use super::gradle::*;

fn temp_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("alfa-atlas-openapi-{nanos}-{n}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Sets up a minimal spec repo whose `specs/responses/all.yaml` refs the
/// well-known common-spec bundle path, without that build artifact
/// actually existing on disk — mirroring a spec repo opened without a
/// prior Gradle build.
fn setup_repo_with_missing_common_spec() -> PathBuf {
    let root = temp_dir();
    fs::create_dir_all(root.join("specs/responses")).unwrap();
    fs::write(
        root.join("specs/api.yaml"),
        "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths: {}\n",
    )
    .unwrap();
    fs::write(
        root.join("specs/responses/all.yaml"),
        "badRequest:\n  $ref: '../../build/common/META-INF/specs/api.yaml#/components/responses/badRequest'\n",
    )
    .unwrap();
    root
}

#[test]
fn falls_back_to_bundled_common_spec_when_enabled() {
    let root = setup_repo_with_missing_common_spec();
    let result = load_openapi_bundle(&root, "specs/api.yaml", true).unwrap();
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got {:?}",
        result.diagnostics
    );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn reports_file_not_found_when_fallback_disabled() {
    let root = setup_repo_with_missing_common_spec();
    // The $ref lives in specs/responses/all.yaml, which the minimal
    // entry document doesn't itself reference; walk it directly to
    // exercise the resolver's file-not-found path.
    let resolver = Resolver {
        repo_root: root.clone(),
        file_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        resolved_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        in_progress: std::cell::RefCell::new(std::collections::HashSet::new()),
        diagnostics: std::cell::RefCell::new(Vec::new()),
        sources: std::cell::RefCell::new(Vec::new()),
        cycle_components: std::cell::RefCell::new(std::collections::HashMap::new()),
        taken_component_names: std::cell::RefCell::new(std::collections::HashSet::new()),
        enable_ref_fallback: false,
    };
    let text = fs::read_to_string(root.join("specs/responses/all.yaml")).unwrap();
    let value = parse_generic(&text, "yaml").unwrap();
    resolver.walk(&value, &root.join("specs/responses/all.yaml"), "");
    let diagnostics = resolver.diagnostics.into_inner();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].reason, "file not found");
    fs::remove_dir_all(&root).ok();
}

#[test]
fn records_the_source_file_of_every_resolved_ref() {
    let root = temp_dir();
    fs::create_dir_all(root.join("specs/operations")).unwrap();
    fs::create_dir_all(root.join("specs/schemas")).unwrap();
    fs::write(
        root.join("specs/api.yaml"),
        concat!(
            "openapi: 3.0.3\n",
            "info:\n  title: t\n  version: '1'\n",
            "paths:\n",
            "  /pets:\n",
            "    get:\n      $ref: './operations/listPets.yaml'\n",
            "    post:\n      $ref: './operations/createPet.yaml'\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join("specs/operations/listPets.yaml"),
        "operationId: listPets\nresponses:\n  '200':\n    description: ok\n    content:\n      application/json:\n        schema:\n          $ref: '../schemas/all.yaml#/Pet'\n",
    )
    .unwrap();
    // Вторая операция ссылается на ту же схему — источник должен быть
    // записан и для неё, несмотря на кэш разрешённых поддеревьев.
    fs::write(
        root.join("specs/operations/createPet.yaml"),
        "operationId: createPet\nrequestBody:\n  content:\n    application/json:\n      schema:\n        $ref: '../schemas/all.yaml#/Pet'\n",
    )
    .unwrap();
    fs::write(
        root.join("specs/schemas/all.yaml"),
        "Pet:\n  type: object\n  properties:\n    name:\n      type: string\n",
    )
    .unwrap();

    let result = load_openapi_bundle(&root, "specs/api.yaml", false).unwrap();
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);

    let source_of = |pointer: &str| {
        result
            .sources
            .iter()
            .find(|s| s.pointer == pointer)
            .unwrap_or_else(|| panic!("no source recorded for {pointer}"))
    };

    assert_eq!(source_of("").file, "specs/api.yaml");
    assert_eq!(source_of("/paths/~1pets/get").file, "specs/operations/listPets.yaml");
    assert_eq!(source_of("/paths/~1pets/get").fragment, "");

    let schema = source_of("/paths/~1pets/get/responses/200/content/application~1json/schema");
    assert_eq!(schema.file, "specs/schemas/all.yaml");
    assert_eq!(schema.fragment, "/Pet");

    let reused =
        source_of("/paths/~1pets/post/requestBody/content/application~1json/schema");
    assert_eq!(reused.file, "specs/schemas/all.yaml");

    fs::remove_dir_all(&root).ok();
}

/// Спека с рекурсивной схемой: `Node.parent` и `Node.children[]` ведут на
/// сам `Node`. Совершенно легальный OpenAPI — генераторы кода собирают из
/// него рекурсивный тип, — поэтому диагностики быть не должно, а сборка
/// обязана остаться валидным документом, годным для генератора.
fn setup_recursive_repo() -> PathBuf {
    let root = temp_dir();
    fs::create_dir_all(root.join("specs/operations")).unwrap();
    fs::create_dir_all(root.join("specs/schemas")).unwrap();
    fs::write(
        root.join("specs/api.yaml"),
        "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\npaths:\n  /nodes:\n    get:\n      $ref: './operations/listNodes.yaml'\n",
    )
    .unwrap();
    fs::write(
        root.join("specs/operations/listNodes.yaml"),
        "operationId: listNodes\nresponses:\n  '200':\n    description: ok\n    content:\n      application/json:\n        schema:\n          $ref: '../schemas/all.yaml#/Node'\n",
    )
    .unwrap();
    fs::write(
        root.join("specs/schemas/all.yaml"),
        concat!(
            "Node:\n",
            "  type: object\n",
            "  properties:\n",
            "    name:\n      type: string\n",
            "    parent:\n      $ref: '#/Node'\n",
            "    children:\n      type: array\n      items:\n        $ref: '#/Node'\n",
        ),
    )
    .unwrap();
    root
}

#[test]
fn recursive_schema_is_hoisted_instead_of_being_reported_as_broken() {
    let root = setup_recursive_repo();
    let result = load_openapi_bundle(&root, "specs/api.yaml", false).unwrap();

    assert!(
        result.diagnostics.is_empty(),
        "рекурсия — не ошибка спеки, got {:?}",
        result.diagnostics
    );

    let schema = result
        .document
        .pointer("/paths/~1nodes/get/responses/200/content/application~1json/schema")
        .unwrap();
    // Внешняя схема развёрнута как обычно...
    assert_eq!(schema.pointer("/properties/name/type").unwrap(), "string");
    // ...а рекурсивные позиции стали нормальными внутренними ссылками.
    assert_eq!(
        schema.pointer("/properties/parent/$ref").unwrap(),
        "#/components/schemas/Node"
    );
    assert_eq!(
        schema.pointer("/properties/children/items/$ref").unwrap(),
        "#/components/schemas/Node"
    );
    // Ссылки ведут на реально существующий узел с телом схемы.
    let hoisted = result
        .document
        .pointer("/components/schemas/Node")
        .expect("рекурсивная схема вынесена в components/schemas");
    assert_eq!(hoisted.pointer("/properties/name/type").unwrap(), "string");
    assert_eq!(
        hoisted.pointer("/properties/parent/$ref").unwrap(),
        "#/components/schemas/Node"
    );
    // Нестандартных ключей вроде `circular` в документе не остаётся.
    assert!(!serde_json::to_string(&result.document)
        .unwrap()
        .contains("\"circular\""));

    fs::remove_dir_all(&root).ok();
}

/// Собирает все `$ref`, оставшиеся в документе после сборки.
fn collect_refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref") {
                out.push(r.clone());
            }
            for v in map.values() {
                collect_refs(v, out);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_refs(v, out);
            }
        }
        _ => {}
    }
}

/// Главное требование к сборке: её можно отдать генератору кода или
/// Swagger UI как один файл. Значит, каждая оставшаяся ссылка обязана
/// разрешаться внутри самого документа — наружу не должно вести ничего.
#[test]
fn bundled_recursive_spec_is_self_contained() {
    let root = setup_recursive_repo();
    let result = load_openapi_bundle(&root, "specs/api.yaml", false).unwrap();

    let mut refs = Vec::new();
    collect_refs(&result.document, &mut refs);
    assert!(!refs.is_empty(), "рекурсия должна оставить ссылки");

    for reference in refs {
        assert!(
            reference.starts_with("#/"),
            "внешняя ссылка {reference} осталась в сборке"
        );
        assert!(
            resolve_pointer(&result.document, reference.trim_start_matches('#')).is_some(),
            "ссылка {reference} никуда не ведёт"
        );
    }

    fs::remove_dir_all(&root).ok();
}

#[test]
fn hoisted_schema_does_not_clobber_one_declared_in_the_entry_document() {
    let root = setup_recursive_repo();
    fs::write(
        root.join("specs/api.yaml"),
        concat!(
            "openapi: 3.0.3\ninfo:\n  title: t\n  version: '1'\n",
            "components:\n  schemas:\n    Node:\n      type: string\n",
            "paths:\n  /nodes:\n    get:\n      $ref: './operations/listNodes.yaml'\n",
        ),
    )
    .unwrap();

    let result = load_openapi_bundle(&root, "specs/api.yaml", false).unwrap();
    assert_eq!(
        result.document.pointer("/components/schemas/Node/type").unwrap(),
        "string",
        "объявленная вручную схема должна уцелеть"
    );
    assert_eq!(
        result
            .document
            .pointer("/paths/~1nodes/get/responses/200/content/application~1json/schema/properties/parent/$ref")
            .unwrap(),
        "#/components/schemas/Node_2"
    );
    assert!(result
        .document
        .pointer("/components/schemas/Node_2/properties/name")
        .is_some());

    fs::remove_dir_all(&root).ok();
}

#[test]
fn is_common_spec_fallback_path_matches_suffix_regardless_of_prefix() {
    assert!(is_common_spec_fallback_path(Path::new(
        "/repo/build/common/META-INF/specs/api.yaml"
    )));
    assert!(!is_common_spec_fallback_path(Path::new(
        "/repo/build/common/META-INF/specs/other.yaml"
    )));
    assert!(!is_common_spec_fallback_path(Path::new(
        "/repo/specs/schemas/api.yaml"
    )));
}

#[test]
fn parse_gradle_common_params_detects_various_formats() {
    assert!(parse_gradle_common_params("openApiConfigurer {\n  commonParams = true\n}"));
    assert!(parse_gradle_common_params("commonParams=true"));
    assert!(parse_gradle_common_params("commonParams = 'true'"));
    assert!(parse_gradle_common_params("commonParams = \"true\""));
    assert!(parse_gradle_common_params("commonParams.set(true)"));
    assert!(parse_gradle_common_params("commonParams(true)"));
    assert!(parse_gradle_common_params("commonParams: true"));
    assert!(parse_gradle_common_params("commonParams true"));
    assert!(parse_gradle_common_params("openApiConfigurer.commonParams.set(true)"));
    assert!(parse_gradle_common_params("openApiConfigurer.commonParams = true"));

    // Full real-world gradle settings example
    let full_example = concat!(
        "plugins {\n",
        "    id 'ru.alfalab.openapi-configurer' version '3.6.0'\n",
        "}\n",
        "openApiConfigurer {\n",
        "    project {\n",
        "        name = \"corp-api-stubs\"\n",
        "        generator = 'spring-web'\n",
        "    }\n",
        "    commonSpecJar = \"ru.alfabank.corp.openapi.common:corp-specs-common:3.13.0\"\n",
        "    commonParams = true\n",
        "}\n",
        "rootProject.name = \"corp-api-specs\"\n",
    );
    let parsed = parse_gradle_openapi_config(full_example);
    assert_eq!(parsed.common_params, Some(true));
    assert_eq!(
        parsed.common_spec_jar.as_deref(),
        Some("ru.alfabank.corp.openapi.common:corp-specs-common:3.13.0")
    );

    // False and disabled cases
    assert!(!parse_gradle_common_params("commonParams = false"));
    assert!(!parse_gradle_common_params("commonParams.set(false)"));
    assert!(!parse_gradle_common_params("// commonParams = true"));
    assert!(!parse_gradle_common_params("/*\ncommonParams = true\n*/"));
    assert!(!parse_gradle_common_params("otherParam = true"));

    // Unrelated task scope should not trigger openApiConfigurer settings
    assert!(!parse_gradle_common_params("tasks.register('custom') {\n  commonParams = true\n}"));
}

#[test]
fn injects_common_headers_when_gradle_common_params_is_true() {
    let root = temp_dir();
    fs::create_dir_all(root.join("specs")).unwrap();
    fs::write(
        root.join("settings.gradle"),
        concat!(
            "plugins {\n",
            "  id 'ru.alfalab.openapi-configurer' version '3.6.0'\n",
            "}\n",
            "openApiConfigurer {\n",
            "  commonParams = true\n",
            "}\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join("specs/api.yaml"),
        concat!(
            "openapi: 3.0.3\n",
            "info:\n  title: Test API\n  version: '1.0.0'\n",
            "paths:\n",
            "  /customers:\n",
            "    get:\n",
            "      summary: List customers\n",
            "      responses:\n",
            "        '200':\n",
            "          description: ok\n",
        ),
    )
    .unwrap();

    let result = load_openapi_bundle(&root, "specs/api.yaml", false).unwrap();
    assert!(result.common_params_enabled);

    let op = result.document.pointer("/paths/~1customers/get").unwrap();
    let params = op.pointer("/parameters").and_then(|v| v.as_array()).unwrap();
    let param_names: Vec<&str> = params
        .iter()
        .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
        .collect();

    assert_eq!(
        param_names,
        vec![
            "A-userId",
            "A-customerId",
            "A-projectId",
            "A-clientType",
            "A-channelId",
            "A-userIp"
        ]
    );

    fs::remove_dir_all(&root).ok();
}

#[test]
fn does_not_clobber_existing_custom_header_when_injecting_common_params() {
    let root = temp_dir();
    fs::create_dir_all(root.join("specs")).unwrap();
    fs::write(
        root.join("settings.gradle.kts"),
        "openApiConfigurer {\n  commonParams.set(true)\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("specs/api.yaml"),
        concat!(
            "openapi: 3.0.3\n",
            "info:\n  title: Test API\n  version: '1.0.0'\n",
            "paths:\n",
            "  /customers:\n",
            "    get:\n",
            "      summary: List customers\n",
            "      parameters:\n",
            "        - name: a-userid\n",
            "          in: header\n",
            "          description: Custom user ID\n",
            "          required: true\n",
            "          schema:\n",
            "            type: string\n",
            "      responses:\n",
            "        '200':\n",
            "          description: ok\n",
        ),
    )
    .unwrap();

    let result = load_openapi_bundle(&root, "specs/api.yaml", false).unwrap();
    assert!(result.common_params_enabled);

    let op = result.document.pointer("/paths/~1customers/get").unwrap();
    let params = op.pointer("/parameters").and_then(|v| v.as_array()).unwrap();

    // Check custom description for a-userid is retained
    let user_id_param = params
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()).map(|n| n.eq_ignore_ascii_case("a-userid")).unwrap_or(false))
        .unwrap();
    assert_eq!(
        user_id_param.get("description").and_then(|d| d.as_str()).unwrap(),
        "Custom user ID"
    );

    // Count A-userId occurrences (should be exactly 1)
    let user_id_count = params
        .iter()
        .filter(|p| p.get("name").and_then(|n| n.as_str()).map(|n| n.eq_ignore_ascii_case("a-userid")).unwrap_or(false))
        .count();
    assert_eq!(user_id_count, 1);

    // Total should be 6 parameters
    assert_eq!(params.len(), 6);

    fs::remove_dir_all(&root).ok();
}
