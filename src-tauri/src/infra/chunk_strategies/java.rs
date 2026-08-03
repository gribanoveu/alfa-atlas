use crate::domain::chunk_index::{spans_from_backward_gap_symbols, ChunkSpan, ChunkStrategy};
use crate::domain::repo_index::{Symbol, SymbolKind};

/// `Method`/`Field` symbols are the chunk anchors; `Class`/`Interface`/
/// `Enum` are deliberately excluded (they're not "bodies," they're the
/// container gaps attach into — see `domain::chunk_index` module docs).
pub struct JavaChunkStrategy;

impl ChunkStrategy for JavaChunkStrategy {
    fn build_spans(&self, symbols: &[Symbol], content_len: usize) -> Vec<ChunkSpan> {
        let anchors: Vec<Symbol> = symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Method | SymbolKind::Field))
            .cloned()
            .collect();
        spans_from_backward_gap_symbols(&anchors, content_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk_index::ChunkKind;

    fn sym(name: &str, kind: SymbolKind, start_byte: u32, end_byte: u32) -> Symbol {
        Symbol {
            name: name.to_string(),
            kind,
            start_line: 1,
            end_line: 1,
            start_byte,
            end_byte,
        }
    }

    #[test]
    fn only_method_and_field_symbols_become_anchors() {
        let class = sym("UserService", SymbolKind::Class, 0, 100);
        let field = sym("repository", SymbolKind::Field, 10, 30);
        let method = sym("save", SymbolKind::Method, 40, 90);
        let spans = JavaChunkStrategy.build_spans(&[class, field, method], 100);

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].kind, ChunkKind::Field);
        assert_eq!(spans[1].kind, ChunkKind::Method);
    }
}
