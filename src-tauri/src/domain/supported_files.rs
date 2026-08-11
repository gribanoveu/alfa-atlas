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

/// Image asset extensions discoverable under docsRoot for `image::` completions.
pub const IMAGE_ASSET_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".bmp", ".ico",
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

pub fn is_image_asset(path: &str) -> bool {
    let ext = extension_of(path);
    IMAGE_ASSET_EXTENSIONS.contains(&ext.as_str())
}

/// Docs-tree / rename-copy allowlist: editable docs plus image assets.
/// Does **not** replace `is_supported_file` for read/write/index.
pub fn is_docs_tree_file(path: &str) -> bool {
    is_supported_file(path) || is_image_asset(path)
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

    #[test]
    fn detects_image_assets() {
        assert!(is_image_asset("images/logo.PNG"));
        assert!(is_image_asset("a.svg"));
        assert!(!is_image_asset("a.adoc"));
        assert!(!is_image_asset("a.rs"));
    }

    #[test]
    fn docs_tree_file_includes_images_but_supported_does_not() {
        assert!(is_docs_tree_file("a.png"));
        assert!(is_docs_tree_file("a.adoc"));
        assert!(!is_supported_file("a.png"));
    }
}
