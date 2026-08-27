//! Types for the Chunk Index layer: splitting an already-indexed file
//! (`domain::repo_index::IndexedFile`) into meaningful, addressable text
//! fragments. No embeddings, no RAG, no vector DB — that's a later stage
//! built on top of this one.
//!
//! A `ChunkStrategy` only knows how to carve a file into ranges (`ChunkSpan`
//! — kind + byte range + which symbol drove it, if any). Turning a span
//! into a full `Chunk` (slicing text, hashing, resolving `qualified_name`,
//! assigning `ordinal`) is `services::chunk_builder::ChunkBuilder`'s job,
//! done once, uniformly, for every language — not duplicated per strategy.

use super::repo_index::{FileId, Language, Symbol, SymbolKind};

/// Bumped whenever the chunking algorithm changes in a way that would
/// reshuffle output for the same input (gap-ownership direction changes,
/// Java gains comment-aware splitting, etc.) — mirrors
/// `repo_index::INDEX_VERSION`.
pub const CHUNK_VERSION: u32 = 1;

/// No tokenizer in this project: a byte-size heuristic decouples this layer
/// from any specific LLM/tokenizer and is easy to swap for a token limit
/// later at the embedding stage.
pub const DEFAULT_MAX_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkBuildOptions {
    pub max_chunk_bytes: usize,
}

impl Default for ChunkBuildOptions {
    fn default() -> Self {
        Self {
            max_chunk_bytes: DEFAULT_MAX_CHUNK_BYTES,
        }
    }
}

/// `"{file_id}#{start_byte}-{end_byte}"` — stable across rebuilds when a
/// file's byte layout hasn't changed, and reads well in logs
/// (`src/UserService.java#512-983`) instead of a raw hash. Change detection
/// is a separate concern, owned by `ChunkMetadata::hash`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkKind {
    Method,
    Field,
    Section,
    File,
}

/// Pure output of a `ChunkStrategy` — a range and what (if anything)
/// anchors it. Everything else a `Chunk` needs (text, hash, id,
/// `qualified_name`, `ordinal`) is filled in afterward by `ChunkBuilder`,
/// which has context (the file's hash, its *full* symbol list) a strategy
/// doesn't need to know about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSpan {
    pub kind: ChunkKind,
    pub start_byte: u32,
    pub end_byte: u32,
    pub anchor_symbol: Option<Symbol>,
}

/// One language's chunking logic — knows only how to carve ranges, nothing
/// about hashing, IDs, or symbol relationships beyond what it's given.
pub trait ChunkStrategy: Send + Sync {
    /// `symbols` is guaranteed sorted by `start_byte` — `ChunkBuilder` sorts
    /// once before calling any strategy, so sort order is never re-derived
    /// (or silently assumed) per language.
    fn build_spans(&self, symbols: &[Symbol], content_len: usize) -> Vec<ChunkSpan>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkMetadata {
    pub id: ChunkId,
    pub file_id: FileId,
    pub language: Language,
    pub kind: ChunkKind,
    pub start_byte: u32,
    pub end_byte: u32,
    /// Which version of the file this chunk was derived from. Stored here,
    /// not just derivable through `RepositoryIndex`, because a `ChunkIndex`
    /// will often outlive or travel separately from the index that built it
    /// (e.g. once chunks are written to a Vector DB) — "is this chunk still
    /// current" must be answerable from `ChunkMetadata` alone.
    pub file_hash: blake3::Hash,
    /// `BLAKE3(file_hash || start_byte || end_byte || CHUNK_VERSION)` — not
    /// a hash of `text`. Changes if the file changes *or* this span's
    /// position shifts, which is the correct trigger for "recompute this
    /// embedding," and avoids re-hashing potentially-large chunk text when
    /// the file hash is already known.
    pub hash: blake3::Hash,
    /// E.g. `Some("UserService.save")` for Java `Method`/`Field` chunks;
    /// `None` for `Section`/`File` chunks — a Markdown/AsciiDoc heading
    /// breadcrumb would need heading-level tracking `Symbol` doesn't carry
    /// yet.
    pub qualified_name: Option<String>,
    /// 0-based position within this file's final chunk sequence (after any
    /// size-limit splitting), assigned by sorting on `start_byte`.
    pub ordinal: u32,
}

/// The actual unit of work for search/AI — unlike `IndexedFile`, a `Chunk`
/// **does** carry its own text. Storing ~16KB-capped fragments (not whole
/// files) is the entire point of this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub metadata: ChunkMetadata,
    pub text: String,
}

fn chunk_kind_for_symbol(kind: SymbolKind) -> ChunkKind {
    match kind {
        SymbolKind::Method => ChunkKind::Method,
        SymbolKind::Field => ChunkKind::Field,
        SymbolKind::Section => ChunkKind::Section,
        // Class/Interface/Enum are never chunk anchors (see module docs) —
        // callers only ever pass Method/Field/Section symbols in here.
        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum => ChunkKind::File,
    }
}

/// Java: `anchors` (already filtered to `Method`/`Field`, already sorted)
/// each capture their own full body already (tree-sitter gives a
/// method/field declaration's complete byte range) — so the only gap to
/// close is *before* each anchor, absorbing annotations/Javadoc/blank
/// lines/package/imports/class declaration backward into the next member's
/// span. The last span additionally absorbs the file's suffix (closing
/// braces). Falls back to `whole_file_span` if there are no anchors at all
/// (e.g. an empty class).
pub fn spans_from_backward_gap_symbols(anchors: &[Symbol], content_len: usize) -> Vec<ChunkSpan> {
    if anchors.is_empty() {
        return whole_file_span(content_len);
    }

    let mut spans = Vec::with_capacity(anchors.len());
    let mut prev_end: u32 = 0;
    let last = anchors.len() - 1;
    for (i, sym) in anchors.iter().enumerate() {
        let end = if i == last {
            content_len as u32
        } else {
            sym.end_byte
        };
        spans.push(ChunkSpan {
            kind: chunk_kind_for_symbol(sym.kind),
            start_byte: prev_end,
            end_byte: end,
            anchor_symbol: Some(sym.clone()),
        });
        prev_end = sym.end_byte;
    }
    spans
}

/// Markdown/AsciiDoc: `anchors` (heading/section-title symbols, already
/// sorted) only span their own title line — the opposite direction from
/// Java: a section's *following* content belongs to it, not the preceding
/// content. The first span's start is forced to `0` (a document preamble
/// attaches forward into the first section); the last span's end is forced
/// to file end. Falls back to `whole_file_span` if there are no headings at
/// all.
pub fn spans_from_forward_gap_symbols(anchors: &[Symbol], content_len: usize) -> Vec<ChunkSpan> {
    if anchors.is_empty() {
        return whole_file_span(content_len);
    }

    let mut spans = Vec::with_capacity(anchors.len());
    let last = anchors.len() - 1;
    for (i, sym) in anchors.iter().enumerate() {
        let start = if i == 0 { 0 } else { sym.start_byte };
        let end = if i == last {
            content_len as u32
        } else {
            anchors[i + 1].start_byte
        };
        spans.push(ChunkSpan {
            kind: chunk_kind_for_symbol(sym.kind),
            start_byte: start,
            end_byte: end,
            anchor_symbol: Some(sym.clone()),
        });
    }
    spans
}

/// JSON/YAML (no symbols at all), or the empty-symbol fallback for any
/// other language: the whole file is one `ChunkKind::File` span. An empty
/// file produces no spans — there's nothing to chunk.
pub fn whole_file_span(content_len: usize) -> Vec<ChunkSpan> {
    if content_len == 0 {
        return Vec::new();
    }
    vec![ChunkSpan {
        kind: ChunkKind::File,
        start_byte: 0,
        end_byte: content_len as u32,
        anchor_symbol: None,
    }]
}

/// Smallest `Class`/`Interface`/`Enum` symbol in `all_symbols` whose range
/// fully contains `anchor`'s — `"{that symbol's name}.{anchor's name}"`.
/// `None` if nothing encloses it (e.g. a `File`-kind span with no anchor at
/// all, or a top-level anchor with no wrapping type).
pub fn qualified_name_for(anchor: &Symbol, all_symbols: &[Symbol]) -> Option<String> {
    all_symbols
        .iter()
        .filter(|s| {
            matches!(s.kind, SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum)
                && s.start_byte <= anchor.start_byte
                && anchor.end_byte <= s.end_byte
        })
        .min_by_key(|s| s.end_byte - s.start_byte)
        .map(|enclosing| format!("{}.{}", enclosing.name, anchor.name))
}

pub fn chunk_hash(file_hash: blake3::Hash, start_byte: u32, end_byte: u32) -> blake3::Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(file_hash.as_bytes());
    hasher.update(&start_byte.to_le_bytes());
    hasher.update(&end_byte.to_le_bytes());
    hasher.update(&CHUNK_VERSION.to_le_bytes());
    hasher.finalize()
}

/// Sorts by `start_byte` and (re)assigns `ordinal` — the one place chunk
/// order is decided, run after semantic splitting *and* size-limit
/// splitting so `ordinal` reflects the final sequence either way.
pub fn finalize_ordinals(mut chunks: Vec<Chunk>) -> Vec<Chunk> {
    chunks.sort_by_key(|c| c.metadata.start_byte);
    for (i, chunk) in chunks.iter_mut().enumerate() {
        chunk.metadata.ordinal = i as u32;
    }
    chunks
}

/// Search window (bytes) `split_one_chunk` looks backward within, from a
/// candidate cut point, for a "safe" boundary before falling back to an
/// arbitrary (but still UTF-8-valid) cut. Not part of `ChunkBuildOptions`
/// — an internal tuning constant, not a public knob.
const SAFE_BOUNDARY_LOOKBACK: usize = 2048;

/// Applied once, uniformly, after semantic splitting — never duplicated per
/// language. A chunk at or under `max_chunk_bytes` passes through
/// unchanged; an oversized one is repeatedly cut near the limit, preferring
/// a blank line, a statement end (`;`), or a closing brace over an
/// arbitrary byte offset. Split parts keep the parent's `kind`/
/// `qualified_name`/`language`/`file_hash`/`file_id` but get their own
/// `id`/`hash` (distinct `start_byte`/`end_byte`); `ordinal` is left at `0`
/// here and fixed up by a subsequent `finalize_ordinals` call regardless of
/// whether anything was split.
pub fn split_oversized_chunks(chunks: Vec<Chunk>, max_chunk_bytes: usize) -> Vec<Chunk> {
    let mut out = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        if chunk.text.len() <= max_chunk_bytes {
            out.push(chunk);
            continue;
        }
        out.extend(split_one_chunk(chunk, max_chunk_bytes));
    }
    out
}

fn split_one_chunk(chunk: Chunk, max_chunk_bytes: usize) -> Vec<Chunk> {
    let Chunk { metadata, text } = chunk;
    let base_start = metadata.start_byte;

    let mut boundaries = Vec::new();
    let mut offset = 0usize;
    while offset < text.len() {
        let remaining = text.len() - offset;
        if remaining <= max_chunk_bytes {
            boundaries.push((offset, text.len()));
            break;
        }
        let cut = find_safe_split(&text, offset, offset + max_chunk_bytes);
        boundaries.push((offset, cut));
        offset = cut;
    }

    boundaries
        .into_iter()
        .map(|(part_start, part_end)| {
            let part_text = text[part_start..part_end].to_string();
            let start_byte = base_start + part_start as u32;
            let end_byte = base_start + part_end as u32;
            Chunk {
                metadata: ChunkMetadata {
                    id: ChunkId(format!("{}#{}-{}", metadata.file_id.0, start_byte, end_byte)),
                    file_id: metadata.file_id.clone(),
                    language: metadata.language,
                    kind: metadata.kind,
                    start_byte,
                    end_byte,
                    file_hash: metadata.file_hash,
                    hash: chunk_hash(metadata.file_hash, start_byte, end_byte),
                    qualified_name: metadata.qualified_name.clone(),
                    ordinal: 0,
                },
                text: part_text,
            }
        })
        .collect()
}

/// Finds a cut point at or before `target` (and after `window_start`),
/// preferring a blank line / statement end / closing-brace line within
/// `SAFE_BOUNDARY_LOOKBACK` bytes of `target`. Always returns a valid UTF-8
/// char boundary. `target` minus `window_start` is at least
/// `max_chunk_bytes` (many times larger than the longest UTF-8 char, 4
/// bytes), so the char-boundary search below is always able to make
/// progress — it can't degrade into a zero-length cut.
fn find_safe_split(text: &str, window_start: usize, target: usize) -> usize {
    let mut safe_target = target.min(text.len());
    while safe_target > window_start && !text.is_char_boundary(safe_target) {
        safe_target -= 1;
    }

    let mut lookback_start = safe_target
        .saturating_sub(SAFE_BOUNDARY_LOOKBACK)
        .max(window_start);
    while lookback_start < safe_target && !text.is_char_boundary(lookback_start) {
        lookback_start += 1;
    }

    let search_area = &text[lookback_start..safe_target];
    if let Some(pos) = search_area.rfind("\n\n") {
        return lookback_start + pos + 2;
    }
    if let Some(pos) = search_area.rfind(";\n") {
        return lookback_start + pos + 2;
    }
    if let Some(pos) = search_area.rfind("}\n") {
        return lookback_start + pos + 2;
    }
    safe_target
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn backward_gap_attaches_prefix_and_gaps_to_the_following_anchor() {
        // "package...;\nclass X {\n  int a;\n\n  void m() {}\n}\n"
        //  0            12         23      31           45  47
        let field = sym("a", SymbolKind::Field, 23, 29);
        let method = sym("m", SymbolKind::Method, 31, 43);
        let spans = spans_from_backward_gap_symbols(&[field.clone(), method.clone()], 47);

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].start_byte, 0);
        assert_eq!(spans[0].end_byte, 29); // field's own end
        assert_eq!(spans[0].kind, ChunkKind::Field);
        assert_eq!(spans[1].start_byte, 29); // gap after field attaches to method
        assert_eq!(spans[1].end_byte, 47); // last span extends to file end
        assert_eq!(spans[1].kind, ChunkKind::Method);
    }

    #[test]
    fn backward_gap_falls_back_to_whole_file_with_no_anchors() {
        let spans = spans_from_backward_gap_symbols(&[], 10);
        assert_eq!(spans, whole_file_span(10));
    }

    #[test]
    fn forward_gap_attaches_preamble_to_first_and_extends_last_to_eof() {
        let h1 = sym("Title", SymbolKind::Section, 0, 8);
        let h2 = sym("Errors", SymbolKind::Section, 20, 30);
        let spans = spans_from_forward_gap_symbols(&[h1, h2], 50);

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].start_byte, 0);
        assert_eq!(spans[0].end_byte, 20); // up to next heading's start
        assert_eq!(spans[1].start_byte, 20);
        assert_eq!(spans[1].end_byte, 50); // extends to file end
    }

    #[test]
    fn whole_file_span_is_empty_for_empty_file() {
        assert!(whole_file_span(0).is_empty());
    }

    #[test]
    fn whole_file_span_covers_the_full_file_otherwise() {
        let spans = whole_file_span(42);
        assert_eq!(spans, vec![ChunkSpan {
            kind: ChunkKind::File,
            start_byte: 0,
            end_byte: 42,
            anchor_symbol: None,
        }]);
    }

    #[test]
    fn qualified_name_finds_the_smallest_enclosing_type() {
        let outer = sym("Outer", SymbolKind::Class, 0, 100);
        let inner = sym("Inner", SymbolKind::Class, 10, 50);
        let method = sym("run", SymbolKind::Method, 15, 20);
        let all = vec![outer, inner, method.clone()];

        assert_eq!(
            qualified_name_for(&method, &all),
            Some("Inner.run".to_string())
        );
    }

    #[test]
    fn qualified_name_is_none_without_an_enclosing_type() {
        let method = sym("run", SymbolKind::Method, 15, 20);
        assert_eq!(qualified_name_for(&method, std::slice::from_ref(&method)), None);
    }

    #[test]
    fn chunk_hash_changes_with_range_and_is_stable_for_the_same_inputs() {
        let file_hash = blake3::hash(b"content");
        let a = chunk_hash(file_hash, 0, 10);
        let b = chunk_hash(file_hash, 0, 11);
        let c = chunk_hash(file_hash, 0, 10);
        assert_ne!(a, b);
        assert_eq!(a, c);
    }

    fn make_chunk(start_byte: u32, text: &str) -> Chunk {
        let file_hash = blake3::hash(b"whatever");
        let end_byte = start_byte + text.len() as u32;
        Chunk {
            metadata: ChunkMetadata {
                id: ChunkId(format!("f#{start_byte}-{end_byte}")),
                file_id: FileId("f".to_string()),
                language: Language::Java,
                kind: ChunkKind::Method,
                start_byte,
                end_byte,
                file_hash,
                hash: chunk_hash(file_hash, start_byte, end_byte),
                qualified_name: Some("X.m".to_string()),
                ordinal: 0,
            },
            text: text.to_string(),
        }
    }

    #[test]
    fn split_oversized_chunks_leaves_small_chunks_untouched() {
        let chunk = make_chunk(0, "small");
        let out = split_oversized_chunks(vec![chunk.clone()], 1024);
        assert_eq!(out, vec![chunk]);
    }

    #[test]
    fn split_oversized_chunks_splits_and_preserves_kind_and_qualified_name() {
        let big_text = "x".repeat(50_000);
        let chunk = make_chunk(1000, &big_text);
        let out = split_oversized_chunks(vec![chunk], 16 * 1024);

        assert!(out.len() > 1);
        for part in &out {
            assert!(part.text.len() <= 16 * 1024);
            assert_eq!(part.metadata.kind, ChunkKind::Method);
            assert_eq!(part.metadata.qualified_name.as_deref(), Some("X.m"));
        }
        // Contiguous, gapless, covers the whole original range.
        assert_eq!(out[0].metadata.start_byte, 1000);
        for pair in out.windows(2) {
            assert_eq!(pair[0].metadata.end_byte, pair[1].metadata.start_byte);
        }
        assert_eq!(out.last().unwrap().metadata.end_byte, 1000 + 50_000);
    }

    #[test]
    fn split_oversized_chunks_prefers_a_blank_line_boundary() {
        // A blank line sits just inside the lookback window before the
        // byte-limit cut point — the split should land there, not mid-line.
        let mut text = "a".repeat(16 * 1024 - 100);
        text.push_str("\n\n");
        text.push_str(&"b".repeat(500));
        let chunk = make_chunk(0, &text);
        let out = split_oversized_chunks(vec![chunk], 16 * 1024);

        assert!(out.len() >= 2);
        let first_end = out[0].metadata.end_byte as usize;
        assert_eq!(&text[first_end - 2..first_end], "\n\n");
    }

    #[test]
    fn finalize_ordinals_sorts_and_numbers_by_position() {
        let c1 = make_chunk(100, "b");
        let c0 = make_chunk(0, "a");
        let out = finalize_ordinals(vec![c1, c0]);
        assert_eq!(out[0].metadata.start_byte, 0);
        assert_eq!(out[0].metadata.ordinal, 0);
        assert_eq!(out[1].metadata.start_byte, 100);
        assert_eq!(out[1].metadata.ordinal, 1);
    }
}
