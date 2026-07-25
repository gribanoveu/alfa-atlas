/// Extensions the editor can open. Keep in sync with `src/lib/supportedFiles.ts`.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    ".adoc",
    ".asciidoc",
    ".json",
    ".md",
    ".markdown",
    ".txt",
    ".puml",
    ".plantuml",
    ".yaml",
    ".yml",
    ".mmd",
    ".mermaid",
];

pub fn extension_of(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let Some(dot) = base.rfind('.') else {
        return String::new();
    };
    if dot == 0 {
        return String::new();
    }
    base[dot..].to_ascii_lowercase()
}

pub fn is_supported_file(path: &str) -> bool {
    let ext = extension_of(path);
    SUPPORTED_EXTENSIONS.contains(&ext.as_str())
}

pub fn is_asciidoc(path: &str) -> bool {
    matches!(extension_of(path).as_str(), ".adoc" | ".asciidoc")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_extensions() {
        assert!(is_supported_file("foo/bar.adoc"));
        assert!(is_supported_file(r"C:\docs\x.JSON"));
        assert!(!is_supported_file("main.rs"));
        assert!(!is_supported_file(".gitignore"));
    }

    #[test]
    fn asciidoc_helpers() {
        assert!(is_asciidoc("a.adoc"));
        assert!(!is_asciidoc("a.md"));
    }
}
