use crate::domain::chunk_index::{spans_from_forward_gap_symbols, ChunkSpan, ChunkStrategy};
use crate::domain::repo_index::{Symbol, SymbolKind};

/// Same shape as `MarkdownChunkStrategy` — AsciiDoc section-title symbols
/// are the anchors, forward gap attachment (heading to next heading).
pub struct AsciiDocChunkStrategy;

impl ChunkStrategy for AsciiDocChunkStrategy {
    fn build_spans(&self, symbols: &[Symbol], content_len: usize) -> Vec<ChunkSpan> {
        let anchors: Vec<Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Section)
            .cloned()
            .collect();
        spans_from_forward_gap_symbols(&anchors, content_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(name: &str, start_byte: u32, end_byte: u32) -> Symbol {
        Symbol {
            name: name.to_string(),
            kind: SymbolKind::Section,
            start_line: 1,
            end_line: 1,
            start_byte,
            end_byte,
        }
    }

    #[test]
    fn each_section_spans_to_the_next_one() {
        let s1 = sym("Guide", 0, 7);
        let s2 = sym("Errors", 30, 39);
        let spans = AsciiDocChunkStrategy.build_spans(&[s1, s2], 60);

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].end_byte, 30);
        assert_eq!(spans[1].end_byte, 60);
    }
}
