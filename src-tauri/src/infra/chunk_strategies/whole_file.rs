use crate::domain::chunk_index::{whole_file_span, ChunkSpan, ChunkStrategy};
use crate::domain::repo_index::Symbol;

/// Used for JSON/YAML (no symbols at all — Repository Index decision 8),
/// and as the empty-symbol fallback other strategies delegate to when a
/// file yields zero leaf anchors (e.g. an empty Java class).
pub struct WholeFileChunkStrategy;

impl ChunkStrategy for WholeFileChunkStrategy {
    fn build_spans(&self, _symbols: &[Symbol], content_len: usize) -> Vec<ChunkSpan> {
        whole_file_span(content_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk_index::ChunkKind;

    #[test]
    fn produces_one_file_kind_span() {
        let spans = WholeFileChunkStrategy.build_spans(&[], 42);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].kind, ChunkKind::File);
        assert_eq!(spans[0].start_byte, 0);
        assert_eq!(spans[0].end_byte, 42);
    }
}
