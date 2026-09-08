//! Java dependency sources: which artifacts a repository declares, where
//! their `-sources.jar` sits in the local Gradle/Maven caches, and unpacking
//! those into one folder the assistant can be handed as a read-only root.
//!
//! The narrowing that keeps this cheap is the manifest itself. A real Spring
//! service resolves to hundreds of transitive artifacts, but declares a few
//! dozen — and the declared ones are what someone actually asks about.
//! Nothing here walks a dependency graph or talks to a build tool: it reads
//! the manifests, looks in the caches the build already populated, and
//! unpacks what it finds.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JavaSourcesError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("не удалось прочитать архив {0}")]
    Archive(String),
    #[error("не найден домашний каталог пользователя")]
    HomeDirUnavailable,
}

/// A Maven coordinate as a manifest spells it. `version` is `None` when the
/// manifest leaves it to a BOM or a property — common in Spring projects,
/// and still resolvable when the cache holds exactly one version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Coordinate {
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
}

/// What one `prepare` run did, for the message the user sees.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PrepareSummary {
    /// Artifacts unpacked by this run.
    pub unpacked: usize,
    /// Artifacts already unpacked by an earlier run and left alone.
    pub reused: usize,
    /// Declared artifacts with no `-sources.jar` in either cache. Normal:
    /// Gradle and Maven only download sources when told to.
    pub without_sources: usize,
}

impl PrepareSummary {
    pub fn available(&self) -> usize {
        self.unpacked + self.reused
    }
}

/// Coordinates declared by the build manifests at `repo_root`.
///
/// Root manifests only — a Gradle multi-project build keeps per-module
/// manifests too, and walking them is a bigger job than it looks (settings
/// parsing, `subprojects {}` blocks). The root file is where the dependencies
/// that matter usually sit; a module's own extras can still be added by hand.
pub fn declared_coordinates(repo_root: &Path) -> Vec<Coordinate> {
    let mut out: Vec<Coordinate> = Vec::new();
    for name in ["build.gradle", "build.gradle.kts"] {
        if let Ok(text) = fs::read_to_string(repo_root.join(name)) {
            out.extend(gradle_coordinates(&text));
        }
    }
    if let Ok(text) = fs::read_to_string(repo_root.join("pom.xml")) {
        out.extend(maven_coordinates(&text));
    }
    out.sort();
    out.dedup();
    out
}

/// Dependency coordinates written as string literals — `"g:a:v"` or `"g:a"`.
///
/// A text scan rather than a walk over `openapi::gradle::tokenize_gradle`'s
/// tokens: both the Groovy and Kotlin DSLs spell a dependency as one string
/// literal, so the literal *is* the grammar worth parsing here, and the same
/// pattern handles `implementation "g:a:v"`, `implementation("g:a:v")` and
/// every `api`/`testImplementation`/`compileOnly` variant without enumerating
/// configuration names.
///
/// The group must contain a dot. Real Maven groups are reverse-DNS, and the
/// requirement is what keeps Gradle's own colon-separated strings (task paths
/// like `":app:build"`, `"org.example"` alone) out of the results. A false
/// positive that survives it simply is not found in the cache.
fn gradle_coordinates(text: &str) -> Vec<Coordinate> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let re = PATTERN.get_or_init(|| {
        Regex::new(r#"["']([A-Za-z0-9_.\-]+\.[A-Za-z0-9_.\-]+):([A-Za-z0-9_.\-]+)(?::([^"'\s]+))?["']"#)
            .expect("static pattern")
    });
    re.captures_iter(text)
        .map(|c| Coordinate {
            group: c[1].to_string(),
            artifact: c[2].to_string(),
            version: c.get(3).map(|m| m.as_str().to_string()).filter(is_literal_version),
        })
        .collect()
}

/// A version the manifest defers to a property (`${springVersion}`, `$libs.x`)
/// is not a version this module can resolve — treated the same as an absent
/// one, so the cache lookup can still succeed when only one version is there.
fn is_literal_version(version: &String) -> bool {
    !version.contains('$') && !version.is_empty()
}

/// `<dependency>` blocks from a `pom.xml`, by tag scan.
///
/// No XML parser: the project has none, and a dependency block is three flat
/// tags with no attributes, namespaces, or nesting to get wrong. Blocks
/// inside `<dependencyManagement>` come along too — those name real
/// artifacts, and unpacking one that the build does not actually use costs a
/// folder nobody opens.
fn maven_coordinates(xml: &str) -> Vec<Coordinate> {
    let mut out = Vec::new();
    for block in xml.split("<dependency>").skip(1) {
        let Some(end) = block.find("</dependency>") else {
            continue;
        };
        let block = &block[..end];
        let (Some(group), Some(artifact)) = (tag_value(block, "groupId"), tag_value(block, "artifactId"))
        else {
            continue;
        };
        out.push(Coordinate {
            group,
            artifact,
            version: tag_value(block, "version").filter(is_literal_version),
        });
    }
    out
}

fn tag_value(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    let value = block[start..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// The `-sources.jar` for `coord` in the local Gradle or Maven cache, if the
/// build ever downloaded one.
pub fn find_sources_jar(home: &Path, coord: &Coordinate) -> Option<(PathBuf, String)> {
    gradle_sources_jar(home, coord).or_else(|| maven_sources_jar(home, coord))
}

/// `~/.gradle/caches/modules-2/files-2.1/{group}/{artifact}/{version}/{hash}/{artifact}-{version}-sources.jar`
/// — the hash directory is why this scans rather than joins a known path.
fn gradle_sources_jar(home: &Path, coord: &Coordinate) -> Option<(PathBuf, String)> {
    let base = home
        .join(".gradle/caches/modules-2/files-2.1")
        .join(&coord.group)
        .join(&coord.artifact);
    let version = resolve_version(&base, coord)?;
    for hash_dir in child_dirs(&base.join(&version)) {
        for entry in fs::read_dir(&hash_dir).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy().ends_with("-sources.jar") {
                return Some((entry.path(), version));
            }
        }
    }
    None
}

/// `~/.m2/repository/{group as path}/{artifact}/{version}/{artifact}-{version}-sources.jar`
/// — a fixed layout, so this joins instead of scanning.
fn maven_sources_jar(home: &Path, coord: &Coordinate) -> Option<(PathBuf, String)> {
    let base = home
        .join(".m2/repository")
        .join(coord.group.replace('.', "/"))
        .join(&coord.artifact);
    let version = resolve_version(&base, coord)?;
    let jar = base
        .join(&version)
        .join(format!("{}-{}-sources.jar", coord.artifact, version));
    jar.is_file().then_some((jar, version))
}

/// The version directory to look in: the declared one when it exists, and
/// otherwise the cache's own answer — but only when the cache holds exactly
/// one version. Picking among several (the newest, say) would quietly show
/// the assistant code from a version this project does not build against,
/// which is worse than showing nothing.
fn resolve_version(base: &Path, coord: &Coordinate) -> Option<String> {
    if let Some(version) = &coord.version {
        return base.join(version).is_dir().then(|| version.clone());
    }
    let mut dirs = child_dirs(base);
    match dirs.len() {
        1 => dirs.pop()?.file_name()?.to_str().map(str::to_string),
        _ => None,
    }
}

fn child_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// Extracts the `.java`/`.kt` entries of a sources jar into `dest`.
///
/// Only source files: a sources jar also carries `META-INF`, resources and
/// the occasional stray binary, none of which is what someone opened the
/// dependency to read, and skipping them keeps the unpacked tree to what the
/// listing should show.
///
/// `enclosed_name` is the archive-traversal guard — it returns `None` for any
/// entry whose path would climb out of the destination (`../../etc/passwd`,
/// an absolute path, a Windows drive prefix), and such an entry is skipped
/// rather than sanitized. A jar is an untrusted archive like any other, even
/// when it came from the user's own cache.
pub fn unpack_sources(jar: &Path, dest: &Path) -> Result<usize, JavaSourcesError> {
    let file = fs::File::open(jar)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| JavaSourcesError::Archive(e.to_string()))?;

    let mut written = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| JavaSourcesError::Archive(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let is_source = relative
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("java") || e.eq_ignore_ascii_case("kt"));
        if !is_source {
            continue;
        }
        let target = dest.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&target)?;
        io::copy(&mut entry, &mut out)?;
        written += 1;
    }
    Ok(written)
}

/// The directory one artifact unpacks into. Named here rather than at the
/// call sites so "is this already unpacked?" and "where do I put it?" cannot
/// answer differently.
fn artifact_dir(artifact: &str, version: &str) -> String {
    format!("{artifact}-{version}")
}

/// Declared artifacts whose sources are cached locally but are not yet
/// unpacked into `dest` — all of the cached ones when nothing has been
/// unpacked yet.
///
/// This is what makes the offer self-renewing: a dependency added to the
/// manifest after the root already exists shows up here, so the Settings
/// list can offer to fetch it instead of the user having to know to remove
/// and re-add the whole root.
pub fn pending_count(repo_root: &Path, dest: Option<&Path>) -> usize {
    let Some(home) = dirs::home_dir() else {
        return 0;
    };
    declared_coordinates(repo_root)
        .iter()
        .filter(|coord| {
            let Some((_, version)) = find_sources_jar(&home, coord) else {
                return false;
            };
            match dest {
                None => true,
                Some(dest) => !dest.join(artifact_dir(&coord.artifact, &version)).is_dir(),
            }
        })
        .count()
}

/// Unpacks the sources of everything `repo_root`'s manifests declare into
/// `dest_root`, one directory per artifact.
///
/// Idempotent by directory: an artifact already unpacked is left alone, so a
/// second run costs a `read_dir` per dependency and nothing else. Artifacts
/// are versioned and immutable, so there is no staleness to check — a
/// dependency that changed version is a different directory.
pub fn prepare(repo_root: &Path, dest_root: &Path) -> Result<PrepareSummary, JavaSourcesError> {
    let home = dirs::home_dir().ok_or(JavaSourcesError::HomeDirUnavailable)?;
    let mut summary = PrepareSummary::default();

    for coord in declared_coordinates(repo_root) {
        let Some((jar, version)) = find_sources_jar(&home, &coord) else {
            summary.without_sources += 1;
            continue;
        };
        let dest = dest_root.join(artifact_dir(&coord.artifact, &version));
        if dest.is_dir() {
            summary.reused += 1;
            continue;
        }
        // Unpack beside the final name and rename into place, so a run
        // interrupted halfway cannot leave a partial directory that the next
        // run would mistake for a finished one.
        let staging = dest_root.join(format!(".{}.partial", artifact_dir(&coord.artifact, &version)));
        fs::remove_dir_all(&staging).ok();
        fs::create_dir_all(&staging)?;
        match unpack_sources(&jar, &staging) {
            Ok(0) => {
                fs::remove_dir_all(&staging).ok();
                summary.without_sources += 1;
            }
            Ok(_) => {
                fs::rename(&staging, &dest)?;
                summary.unpacked += 1;
            }
            Err(e) => {
                fs::remove_dir_all(&staging).ok();
                eprintln!("[java-sources] {}: {e}", coord.artifact);
                summary.without_sources += 1;
            }
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("alfa-atlas-java-{label}-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Builds a sources jar whose entries are given as `(name, contents)`.
    /// Entry names go in verbatim, which is what lets a test put a
    /// directory-traversal name in one.
    fn write_jar(path: &Path, entries: &[(&str, &str)]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, contents) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            io::Write::write_all(&mut zip, contents.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn gradle_dependencies_are_read_from_both_dsls() {
        let text = r#"
            dependencies {
                implementation 'com.fasterxml.jackson.core:jackson-databind:2.15.0'
                implementation("org.springframework.boot:spring-boot-starter-web:3.2.0")
                testImplementation("org.assertj:assertj-core")
                implementation "org.example.lib:widget:${widgetVersion}"
                // Not dependencies: a task path and a plain group name.
                tasks.named(":app:build")
                group = "com.acme"
            }
        "#;

        let found = gradle_coordinates(text);

        assert!(found.contains(&Coordinate {
            group: "com.fasterxml.jackson.core".into(),
            artifact: "jackson-databind".into(),
            version: Some("2.15.0".into()),
        }));
        assert!(found.contains(&Coordinate {
            group: "org.springframework.boot".into(),
            artifact: "spring-boot-starter-web".into(),
            version: Some("3.2.0".into()),
        }));
        // No version in the manifest — resolvable later only if the cache is
        // unambiguous, which is the point of carrying `None` rather than
        // dropping the line.
        assert!(found.contains(&Coordinate {
            group: "org.assertj".into(),
            artifact: "assertj-core".into(),
            version: None,
        }));
        // A property version is not a version this module can resolve.
        assert!(found.contains(&Coordinate {
            group: "org.example.lib".into(),
            artifact: "widget".into(),
            version: None,
        }));
        assert!(!found.iter().any(|c| c.artifact == "build"));
    }

    #[test]
    fn maven_dependencies_are_read_by_tag_scan() {
        let xml = r#"
            <project>
              <dependencies>
                <dependency>
                  <groupId>com.fasterxml.jackson.core</groupId>
                  <artifactId>jackson-databind</artifactId>
                  <version>2.15.0</version>
                </dependency>
                <dependency>
                  <groupId>org.slf4j</groupId>
                  <artifactId>slf4j-api</artifactId>
                  <version>${slf4j.version}</version>
                </dependency>
              </dependencies>
            </project>
        "#;

        let found = maven_coordinates(xml);

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].version.as_deref(), Some("2.15.0"));
        assert_eq!(found[1].artifact, "slf4j-api");
        assert_eq!(found[1].version, None, "a property version is not resolvable here");
    }

    /// A jar is an untrusted archive: an entry that would climb out of the
    /// destination is skipped, not written somewhere else.
    #[test]
    fn unpacking_takes_sources_only_and_refuses_to_escape() {
        let dir = temp_dir("unpack");
        let jar = dir.join("lib-sources.jar");
        let dest = dir.join("out");
        fs::create_dir_all(&dest).unwrap();
        write_jar(
            &jar,
            &[
                ("com/acme/Client.java", "class Client {}\n"),
                ("com/acme/Client.kt", "class Client\n"),
                ("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n"),
                ("com/acme/logo.png", "not source\n"),
                ("../escaped.java", "class Escaped {}\n"),
            ],
        );

        let written = unpack_sources(&jar, &dest).unwrap();

        assert_eq!(written, 2, "only the .java and .kt entries");
        assert!(dest.join("com/acme/Client.java").is_file());
        assert!(dest.join("com/acme/Client.kt").is_file());
        assert!(!dest.join("META-INF/MANIFEST.MF").exists());
        assert!(!dir.join("escaped.java").exists(), "traversal must not land outside dest");

        fs::remove_dir_all(&dir).ok();
    }

    /// The whole flow against a fake Gradle cache under a temp `$HOME`:
    /// manifest → cache lookup → unpack, and a second run that touches
    /// nothing.
    #[test]
    fn prepare_unpacks_declared_artifacts_once() {
        crate::infra::settings_store::test_support::with_temp_home(|| {
            let home = dirs::home_dir().unwrap();
            let repo = temp_dir("repo");
            let dest = temp_dir("dest");

            fs::write(
                repo.join("build.gradle"),
                "dependencies {\n  implementation 'com.acme.tools:widget:1.4.0'\n  implementation 'com.acme.tools:missing:9.9.9'\n}\n",
            )
            .unwrap();

            let cached = home
                .join(".gradle/caches/modules-2/files-2.1/com.acme.tools/widget/1.4.0/abc123");
            fs::create_dir_all(&cached).unwrap();
            write_jar(
                &cached.join("widget-1.4.0-sources.jar"),
                &[("com/acme/Widget.java", "class Widget {}\n")],
            );

            let first = prepare(&repo, &dest).unwrap();
            assert_eq!(first.unpacked, 1);
            assert_eq!(first.reused, 0);
            assert_eq!(first.without_sources, 1, "the artifact with no cached jar");
            assert!(dest.join("widget-1.4.0/com/acme/Widget.java").is_file());

            let second = prepare(&repo, &dest).unwrap();
            assert_eq!(second.unpacked, 0);
            assert_eq!(second.reused, 1);

            fs::remove_dir_all(&repo).ok();
            fs::remove_dir_all(&dest).ok();
        });
    }

    /// A manifest that leaves the version to a BOM still resolves, but only
    /// while the cache's answer is unambiguous.
    #[test]
    fn a_missing_version_resolves_only_when_the_cache_holds_one() {
        crate::infra::settings_store::test_support::with_temp_home(|| {
            let home = dirs::home_dir().unwrap();
            let base = home.join(".gradle/caches/modules-2/files-2.1/com.acme.tools/widget");
            fs::create_dir_all(base.join("1.4.0/abc")).unwrap();
            write_jar(
                &base.join("1.4.0/abc/widget-1.4.0-sources.jar"),
                &[("com/acme/Widget.java", "class Widget {}\n")],
            );
            let coord = Coordinate {
                group: "com.acme.tools".into(),
                artifact: "widget".into(),
                version: None,
            };

            let (_, version) = find_sources_jar(&home, &coord).expect("one version is unambiguous");
            assert_eq!(version, "1.4.0");

            // A second cached version makes the answer a guess, so there is
            // no answer.
            fs::create_dir_all(base.join("2.0.0/def")).unwrap();
            assert!(find_sources_jar(&home, &coord).is_none());
        });
    }
}
