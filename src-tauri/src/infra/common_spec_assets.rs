//! Built-in default copy of the "common" OpenAPI spec bundle, embedded at
//! compile time.
//!
//! Many spec repositories (built on the `ru.alfalab.openapi-configurer` +
//! `commonSpecJar` Gradle convention) reference a shared bundle of schemas
//! and responses (`Currency`, `Amount`, `ResponseError`, `badRequest`,
//! `notFound`, ...) via a relative `$ref` to `build/common/META-INF/specs/api.yaml`.
//! That path is a Java/Gradle build artifact — it's always gitignored and only
//! materializes after a Gradle build extracts it from the published jar. When
//! a spec repo is opened here without that build step having run, those refs
//! would otherwise show as unresolved. Embedding (rather than shipping as
//! Tauri bundle resources) sidesteps the resource-path differences between
//! `cargo tauri dev` and a bundled build — the data is simply part of the
//! binary, in dev and in production alike (see `dictionary_assets.rs` for the
//! same rationale applied to spellcheck dictionaries).

const COMMON_API_YAML: &str = include_str!("../../assets/common-spec/api.yaml");

/// Raw YAML source of the bundled default common spec bundle.
pub fn bundled_common_api_yaml() -> &'static str {
    COMMON_API_YAML
}
