//! Heading extraction for Markdown, via `pulldown-cmark`.
//!
//! Separate from `infra/parsers/markdown.rs::parse` (which only records a
//! heading as an `Anchor` when it has an explicit `{ #id }` attribute, for
//! the editor's cross-reference index) — this indexer records every
//! heading, with byte ranges, regardless of whether it has an id, since a
//! "symbol" here just means "named section", not "linkable anchor".

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::domain::repo_index::{LanguageFacts, LanguageIndexer, Symbol, SymbolKind};

pub struct MarkdownIndexer;

impl LanguageIndexer for MarkdownIndexer {
    fn index(&self, content: &str) -> LanguageFacts {
        let opts = Options::ENABLE_TABLES | Options::ENABLE_HEADING_ATTRIBUTES;
        let parser = Parser::new_ext(content, opts);
        let line_starts = build_line_starts(content);

        let mut symbols = Vec::new();
        let mut current: Option<(usize, String)> = None;

        for (event, range) in parser.into_offset_iter() {
            match event {
                Event::Start(Tag::Heading { .. }) => {
                    current = Some((range.start, String::new()));
                }
                Event::Text(text) | Event::Code(text) => {
                    if let Some((_, buf)) = current.as_mut() {
                        buf.push_str(&text);
                    }
                }
                Event::End(TagEnd::Heading(_)) => {
                    if let Some((start, name)) = current.take() {
                        let name = name.trim().to_string();
                        if !name.is_empty() {
                            symbols.push(Symbol {
                                name,
                                kind: SymbolKind::Section,
                                start_line: line_for(start, &line_starts),
                                end_line: line_for(range.end.saturating_sub(1), &line_starts),
                                start_byte: start as u32,
                                end_byte: range.end as u32,
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        LanguageFacts { symbols, imports: Vec::new() }
    }
}

fn build_line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (idx, ch) in content.char_indices() {
        if ch == '\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn line_for(byte_offset: usize, line_starts: &[usize]) -> u32 {
    let idx = line_starts
        .binary_search(&byte_offset)
        .unwrap_or_else(|i| i.saturating_sub(1));
    (idx as u32) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_headings_at_multiple_levels() {
        let facts = MarkdownIndexer.index("# Title\n\ntext\n\n## Sub `Section`\n\nmore\n");
        assert_eq!(facts.symbols.len(), 2);
        assert_eq!(facts.symbols[0].name, "Title");
        assert_eq!(facts.symbols[0].kind, SymbolKind::Section);
        assert_eq!(facts.symbols[0].start_line, 1);
        assert_eq!(facts.symbols[1].name, "Sub Section");
        assert_eq!(facts.symbols[1].start_line, 5);
        assert!(facts.symbols[1].start_byte < facts.symbols[1].end_byte);
    }

    #[test]
    fn ignores_documents_without_headings() {
        let facts = MarkdownIndexer.index("just text\n\nmore text\n");
        assert!(facts.symbols.is_empty());
    }
}
