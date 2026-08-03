use crate::domain::chunk_index::{spans_from_forward_gap_symbols, ChunkSpan, ChunkStrategy};
use crate::domain::repo_index::{Symbol, SymbolKind};

/// `Section` (heading) symbols are the anchors; each chunk spans from its
/// heading to the next one (or file end) — see `domain::chunk_index`
/// module docs for why headings use forward gap attachment while Java uses
/// backward.
pub struct MarkdownChunkStrategy;

impl ChunkStrategy for MarkdownChunkStrategy {
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
    fn each_heading_spans_to_the_next_one() {
        let h1 = sym("Title", 0, 7);
        let h2 = sym("Errors", 30, 39);
        let spans = MarkdownChunkStrategy.build_spans(&[h1, h2], 60);

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].start_byte, 0);
        assert_eq!(spans[0].end_byte, 30);
        assert_eq!(spans[1].start_byte, 30);
        assert_eq!(spans[1].end_byte, 60);
    }
}
