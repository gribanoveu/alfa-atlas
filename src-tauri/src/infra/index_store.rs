//! SQLite-backed durable mirror of `ChunkIndex`/`EmbeddingIndex` metadata —
//! everything needed to reload both without a full repo rescan, and to
//! diff incrementally against what's on disk now. Deliberately stores no
//! embedding vectors (they live in `vectors.usearch`, see
//! `infra::vector_store`) — the relational tables here are only ids, byte
//! offsets, and hashes.
//!
//! The one exception is `chunks_fts`, the FTS5 index behind the BM25 search
//! tier: a full-text index *is* a copy of the text, there is no way to rank
//! by term statistics without one, and the alternative the lexical tier
//! used before it — reading and lowercasing every chunk in the repository
//! on every query — cost far more than the disk this does. The relational
//! tables still hold no text, and `chunk_text::resolve_text` remains the
//! only way a *result* gets its snippet.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::UNIX_EPOCH;

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

use crate::domain::chunk_index::{Chunk, ChunkId, ChunkKind, ChunkMetadata};
use crate::domain::repo_index::{FileId, FileMetadata, ImportRef, Language, Symbol, SymbolKind};

const DB_FILE_NAME: &str = "chunks.db";
pub const VECTORS_FILE_NAME: &str = "vectors.usearch";

/// Bump whenever the FTS schema or tokenizer changes in a way that makes
/// already-indexed rows wrong. Deliberately *not* folded into
/// `CHUNK_VERSION`/`INDEX_VERSION`: those invalidate the whole store,
/// vectors included, and re-embedding a repository costs hours — a
/// full-text index can be rebuilt from the source files in seconds, so it
/// carries its own version and its own repair (see
/// `embedding_sync::backfill_fts`).
const FTS_VERSION: &str = "1";
const META_FTS_VERSION: &str = "fts_version";

const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
  file_id    TEXT PRIMARY KEY,
  file_hash  BLOB NOT NULL,
  size_bytes INTEGER NOT NULL,
  mtime_secs INTEGER NOT NULL,
  language   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chunks (
  chunk_id       TEXT PRIMARY KEY,
  file_id        TEXT NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
  language       TEXT NOT NULL,
  kind           TEXT NOT NULL,
  start_byte     INTEGER NOT NULL,
  end_byte       INTEGER NOT NULL,
  file_hash      BLOB NOT NULL,
  chunk_hash     BLOB NOT NULL,
  qualified_name TEXT,
  ordinal        INTEGER NOT NULL,
  -- This chunk's row in `chunks_fts`. A virtual table takes no foreign key
  -- and so is not reached by the `ON DELETE CASCADE` above; keeping the
  -- rowid here is what lets a delete find its FTS row through
  -- `idx_chunks_file_id` instead of scanning the whole full-text index.
  -- Nullable: an index written before the FTS tier existed has chunk rows
  -- with no counterpart until `replace_fts_for_file` backfills them.
  fts_rowid      INTEGER
);
CREATE INDEX IF NOT EXISTS idx_chunks_file_id ON chunks(file_id);

-- The BM25 tier's index. `unicode61` case-folds Cyrillic but does not stem
-- it, which is why `domain::search_query::fts5_query` cuts longer terms to
-- a stem and searches them as prefixes. `prefix = '4 5 6'` covers exactly
-- the lengths that produces (see `FTS_STEM_PREFIX_CHARS`), so those lookups
-- hit an index instead of walking the term list — keep the two in step.
-- `qualified_name` is indexed alongside the text (and weighted above it at
-- query time) so an identifier query lands on the method that carries the
-- name, not just on prose mentioning it.
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
  chunk_id UNINDEXED,
  qualified_name,
  text,
  tokenize = 'unicode61 remove_diacritics 2',
  prefix = '4 5 6'
);

-- `chunk_hash` here mirrors `EmbeddingRecord.chunk_hash`, written only once
-- a vector actually lands in `vectors.usearch` — deliberately a separate
-- write from `chunks.chunk_hash` (see module docs on `EmbeddingIndex::sync`
-- for why that gap is a crash-safety feature, not a bug).
CREATE TABLE IF NOT EXISTS embeddings (
  chunk_id   TEXT PRIMARY KEY REFERENCES chunks(chunk_id) ON DELETE CASCADE,
  chunk_hash BLOB NOT NULL
);

-- One row per `Symbol` a `LanguageIndexer` extracted, mirroring
-- `RepositoryIndex`'s in-memory `IndexedFile.symbols` so a cold start can
-- reuse a file's already-parsed symbols (via `RepositoryIndex::
-- build_reusing_symbols`) instead of re-running tree-sitter/pulldown-cmark
-- on every file, every time — gated on `files.file_hash` still matching,
-- same as chunks/embeddings. No primary key of its own; nothing references
-- a symbol by id, only by `file_id`.
CREATE TABLE IF NOT EXISTS symbols (
  file_id    TEXT NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  kind       TEXT NOT NULL,
  start_line INTEGER NOT NULL,
  end_line   INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id);

-- One row per `ImportRef` a `JavaIndexer` extracted, mirroring
-- `RepositoryIndex`'s in-memory `IndexedFile.imports` — persisted so a cold
-- start's `RepositoryIndex::build_reusing_symbols` can carry a reused (i.e.
-- content-unchanged) file's import graph forward instead of silently
-- dropping it (see `services::embedding_sync::load_persisted_symbols`). Same
-- shape/gating as `symbols`: no primary key, keyed by `file_id`.
CREATE TABLE IF NOT EXISTS imports (
  file_id     TEXT NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
  fqn         TEXT NOT NULL,
  is_wildcard INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_imports_file_id ON imports(file_id);
"#;

#[derive(Debug, Error)]
pub enum IndexStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
    #[error("index store lock poisoned")]
    LockPoisoned,
}

/// One SQLite connection per `index_root`, shared by `ChunkIndex` and
/// `EmbeddingIndex` for that project — both write through it from their
/// own mutating methods rather than a caller having to remember to keep a
/// separate bookkeeping layer in sync.
pub struct IndexStore {
    conn: Mutex<Connection>,
    dir: PathBuf,
}

impl IndexStore {
    /// Opens (creating if needed) `{index_dir}/chunks.db`, applying the
    /// schema idempotently. `index_dir` is conventionally
    /// `{repo_root}/.atlas/index`.
    pub fn open(index_dir: &Path) -> Result<Self, IndexStoreError> {
        std::fs::create_dir_all(index_dir).map_err(IndexStoreError::Io)?;
        let conn = Connection::open(index_dir.join(DB_FILE_NAME))?;
        conn.execute_batch(SCHEMA_SQL)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            dir: index_dir.to_path_buf(),
        })
    }

    pub fn vectors_path(&self) -> PathBuf {
        self.dir.join(VECTORS_FILE_NAME)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, IndexStoreError> {
        self.conn.lock().map_err(|_| IndexStoreError::LockPoisoned)
    }

    pub fn read_meta(&self, key: &str) -> Result<Option<String>, IndexStoreError> {
        let conn = self.lock()?;
        Ok(conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn write_meta(&self, key: &str, value: &str) -> Result<(), IndexStoreError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Deletes every row (`meta`/`files`/`chunks`/`chunks_fts`/`embeddings`/
    /// `symbols`/`imports`) — used when the version/`index_root`
    /// compatibility guard decides a persisted store is unusable and must be
    /// rebuilt from scratch.
    pub fn wipe(&self) -> Result<(), IndexStoreError> {
        let conn = self.lock()?;
        conn.execute_batch(
            "DELETE FROM embeddings; DELETE FROM chunks; DELETE FROM chunks_fts; DELETE FROM symbols; DELETE FROM imports; DELETE FROM files; DELETE FROM meta;",
        )?;
        Ok(())
    }

    pub fn upsert_files(&self, files: &[FileMetadata]) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        for file in files {
            let mtime_secs = file
                .modified_at
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            tx.execute(
                "INSERT INTO files (file_id, file_hash, size_bytes, mtime_secs, language)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(file_id) DO UPDATE SET
                    file_hash = excluded.file_hash,
                    size_bytes = excluded.size_bytes,
                    mtime_secs = excluded.mtime_secs,
                    language = excluded.language",
                params![
                    file.relative_path,
                    file.hash.as_bytes().to_vec(),
                    file.size_bytes as i64,
                    mtime_secs,
                    language_to_str(file.language),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Cascades to `chunks`/`embeddings` via the `ON DELETE CASCADE` FKs.
    /// `chunks_fts` is a virtual table and takes no foreign key, so its rows
    /// are dropped explicitly first — while `chunks` still holds the
    /// `fts_rowid`s that locate them.
    pub fn delete_files(&self, file_ids: &[FileId]) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        for file_id in file_ids {
            purge_fts_rows(&tx, file_id)?;
            tx.execute("DELETE FROM files WHERE file_id = ?1", params![file_id.0])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Drops every existing chunk row for `file_id`, then inserts `chunks`
    /// — mirrors `ChunkIndex::replace_for_file`, which calls this in
    /// lockstep. Cascades to that file's `embeddings` rows too.
    ///
    /// Takes whole `Chunk`s rather than the `ChunkMetadata` it used to,
    /// because `chunks_fts` needs the text and this is the only place it is
    /// still in hand — the caller is about to hand the same `Vec` to
    /// `ChunkIndex::replace_for_file`, which drops it. One write path for
    /// both tables is the point: an FTS index that can silently disagree
    /// with `chunks` about which chunks exist is worse than none.
    pub fn replace_chunks_for_file(
        &self,
        file_id: &FileId,
        chunks: &[Chunk],
    ) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        purge_fts_rows(&tx, file_id)?;
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id.0])?;
        for chunk in chunks {
            let meta = &chunk.metadata;
            let fts_rowid = insert_fts_row(&tx, &meta.id, meta.qualified_name.as_deref(), &chunk.text)?;
            tx.execute(
                "INSERT INTO chunks (chunk_id, file_id, language, kind, start_byte, end_byte,
                    file_hash, chunk_hash, qualified_name, ordinal, fts_rowid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    meta.id.0,
                    meta.file_id.0,
                    language_to_str(meta.language),
                    chunk_kind_to_str(meta.kind),
                    meta.start_byte,
                    meta.end_byte,
                    meta.file_hash.as_bytes().to_vec(),
                    meta.hash.as_bytes().to_vec(),
                    meta.qualified_name,
                    meta.ordinal,
                    fts_rowid,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Re-indexes `file_id`'s existing chunks for full-text search without
    /// touching `chunks` itself. The backfill path (see
    /// `embedding_sync::backfill_fts`): an index written before this tier
    /// existed already has correct chunk rows and, more to the point,
    /// `embeddings` rows that took hours to compute — going through
    /// `replace_chunks_for_file` would cascade those away for nothing.
    ///
    /// `entries` is `(chunk_id, qualified_name, text)`, in any order; ids
    /// that no longer have a `chunks` row are indexed but left unlinked,
    /// and the next `replace_chunks_for_file` for the file clears them.
    pub fn replace_fts_for_file(
        &self,
        file_id: &FileId,
        entries: &[(ChunkId, Option<String>, String)],
    ) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        purge_fts_rows(&tx, file_id)?;
        for (chunk_id, qualified_name, text) in entries {
            let fts_rowid = insert_fts_row(&tx, chunk_id, qualified_name.as_deref(), text)?;
            tx.execute(
                "UPDATE chunks SET fts_rowid = ?1 WHERE chunk_id = ?2",
                params![fts_rowid, chunk_id.0],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Whether `chunks_fts` predates the running binary's `FTS_VERSION` —
    /// true for any index written before this tier existed, and after any
    /// tokenizer change. The caller rebuilds and then calls
    /// `mark_fts_current`.
    pub fn fts_needs_backfill(&self) -> Result<bool, IndexStoreError> {
        Ok(self.read_meta(META_FTS_VERSION)?.as_deref() != Some(FTS_VERSION))
    }

    pub fn mark_fts_current(&self) -> Result<(), IndexStoreError> {
        self.write_meta(META_FTS_VERSION, FTS_VERSION)
    }

    /// The BM25 tier: ranks `chunks_fts` against an FTS5 `MATCH` expression
    /// from `domain::search_query::fts5_query`, best first.
    ///
    /// `bm25()` scores lower-is-better (it returns a negated value), which
    /// is why the ordering is ascending and the score is flipped on the way
    /// out — every other tier here reports higher-is-better, and
    /// `apply_related_boost` multiplies, so a negative score would invert
    /// the boost into a penalty.
    pub fn search_bm25(
        &self,
        fts_query: &str,
        limit: usize,
    ) -> Result<Vec<(ChunkId, f32)>, IndexStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            // Column weights, positionally: `chunk_id` is UNINDEXED and can
            // never match (0.0), `qualified_name` counts for four times a
            // body-text hit, `text` is the baseline.
            "SELECT chunk_id, bm25(chunks_fts, 0.0, 4.0, 1.0) AS rank
             FROM chunks_fts
             WHERE chunks_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
            let chunk_id: String = row.get(0)?;
            let rank: f64 = row.get(1)?;
            Ok((ChunkId(chunk_id), -rank as f32))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(IndexStoreError::from)
    }

    /// Every persisted chunk's metadata — what `ChunkIndex::ensure_loaded`
    /// bulk-populates its resident `DashMap` from on cold start, instead of
    /// a full repo rescan.
    pub fn load_all_chunks(&self) -> Result<Vec<ChunkMetadata>, IndexStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT chunk_id, file_id, language, kind, start_byte, end_byte,
                    file_hash, chunk_hash, qualified_name, ordinal
             FROM chunks",
        )?;
        let rows = stmt.query_map([], |row| {
            let file_hash_bytes: Vec<u8> = row.get(6)?;
            let chunk_hash_bytes: Vec<u8> = row.get(7)?;
            let language = str_to_language(&row.get::<_, String>(2)?).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(2, "language".into(), rusqlite::types::Type::Text)
            })?;
            let kind = str_to_chunk_kind(&row.get::<_, String>(3)?).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(3, "kind".into(), rusqlite::types::Type::Text)
            })?;
            Ok(ChunkMetadata {
                id: ChunkId(row.get(0)?),
                file_id: FileId(row.get(1)?),
                language,
                kind,
                start_byte: row.get(4)?,
                end_byte: row.get(5)?,
                file_hash: hash_from_bytes(&file_hash_bytes),
                hash: hash_from_bytes(&chunk_hash_bytes),
                qualified_name: row.get(8)?,
                ordinal: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(IndexStoreError::from)
    }

    pub fn upsert_embedding(
        &self,
        chunk_id: &ChunkId,
        chunk_hash: blake3::Hash,
    ) -> Result<(), IndexStoreError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO embeddings (chunk_id, chunk_hash) VALUES (?1, ?2)
             ON CONFLICT(chunk_id) DO UPDATE SET chunk_hash = excluded.chunk_hash",
            params![chunk_id.0, chunk_hash.as_bytes().to_vec()],
        )?;
        Ok(())
    }

    pub fn delete_embedding(&self, chunk_id: &ChunkId) -> Result<(), IndexStoreError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM embeddings WHERE chunk_id = ?1", params![chunk_id.0])?;
        Ok(())
    }

    pub fn clear_embeddings(&self) -> Result<(), IndexStoreError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM embeddings", [])?;
        Ok(())
    }

    /// What `EmbeddingIndex::ensure_loaded`/`load` reconstructs its
    /// resident `records` map from — pairs with `vectors.usearch` (loaded
    /// separately by `VectorStore::load`) to fully restore the index
    /// without re-embedding anything that hasn't changed.
    pub fn load_all_embedding_hashes(&self) -> Result<Vec<(ChunkId, blake3::Hash)>, IndexStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT chunk_id, chunk_hash FROM embeddings")?;
        let rows = stmt.query_map([], |row| {
            let chunk_id: String = row.get(0)?;
            let hash_bytes: Vec<u8> = row.get(1)?;
            Ok((chunk_id, hash_bytes))
        })?;
        rows.map(|r| r.map(|(id, bytes)| (ChunkId(id), hash_from_bytes(&bytes))))
            .collect::<Result<Vec<_>, _>>()
            .map_err(IndexStoreError::from)
    }

    /// Drops every existing symbol row for `file_id`, then inserts
    /// `symbols` — mirrors `replace_chunks_for_file`'s "delete then bulk
    /// insert" shape. Callers write this in lockstep with `upsert_files`,
    /// same as chunks, whenever a file's parse result actually changed.
    pub fn replace_symbols_for_file(
        &self,
        file_id: &FileId,
        symbols: &[Symbol],
    ) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM symbols WHERE file_id = ?1", params![file_id.0])?;
        for symbol in symbols {
            tx.execute(
                "INSERT INTO symbols (file_id, name, kind, start_line, end_line, start_byte, end_byte)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    file_id.0,
                    symbol.name,
                    symbol_kind_to_str(symbol.kind),
                    symbol.start_line,
                    symbol.end_line,
                    symbol.start_byte,
                    symbol.end_byte,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Every persisted symbol, grouped by the file it belongs to — what
    /// `RepositoryIndex::build_reusing_symbols` reuses for a file whose
    /// current content hash still matches `load_all_file_hashes`' record,
    /// instead of re-parsing it.
    pub fn load_all_symbols(&self) -> Result<HashMap<FileId, Vec<Symbol>>, IndexStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT file_id, name, kind, start_line, end_line, start_byte, end_byte FROM symbols",
        )?;
        let rows = stmt.query_map([], |row| {
            let file_id: String = row.get(0)?;
            let kind = str_to_symbol_kind(&row.get::<_, String>(2)?).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(2, "kind".into(), rusqlite::types::Type::Text)
            })?;
            Ok((
                FileId(file_id),
                Symbol {
                    name: row.get(1)?,
                    kind,
                    start_line: row.get(3)?,
                    end_line: row.get(4)?,
                    start_byte: row.get(5)?,
                    end_byte: row.get(6)?,
                },
            ))
        })?;

        let mut out: HashMap<FileId, Vec<Symbol>> = HashMap::new();
        for row in rows {
            let (file_id, symbol) = row?;
            out.entry(file_id).or_default().push(symbol);
        }
        Ok(out)
    }

    /// Drops every existing import row for `file_id`, then inserts
    /// `imports` — mirrors `replace_symbols_for_file`'s "delete then bulk
    /// insert" shape exactly.
    pub fn replace_imports_for_file(
        &self,
        file_id: &FileId,
        imports: &[ImportRef],
    ) -> Result<(), IndexStoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM imports WHERE file_id = ?1", params![file_id.0])?;
        for import in imports {
            tx.execute(
                "INSERT INTO imports (file_id, fqn, is_wildcard) VALUES (?1, ?2, ?3)",
                params![file_id.0, import.fqn, import.is_wildcard],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Every persisted import, grouped by the file it belongs to — what
    /// `services::embedding_sync::load_persisted_symbols` reuses so a cold-start
    /// `RepositoryIndex::build_reusing_symbols` call carries a reused file's
    /// Java import graph forward instead of losing it.
    pub fn load_all_imports(&self) -> Result<HashMap<FileId, Vec<ImportRef>>, IndexStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT file_id, fqn, is_wildcard FROM imports")?;
        let rows = stmt.query_map([], |row| {
            let file_id: String = row.get(0)?;
            Ok((
                FileId(file_id),
                ImportRef {
                    fqn: row.get(1)?,
                    is_wildcard: row.get(2)?,
                },
            ))
        })?;

        let mut out: HashMap<FileId, Vec<ImportRef>> = HashMap::new();
        for row in rows {
            let (file_id, import) = row?;
            out.entry(file_id).or_default().push(import);
        }
        Ok(out)
    }

    /// Every persisted file's full metadata (content hash, size, mtime,
    /// language) — the other half (alongside `load_all_symbols`)
    /// `RepositoryIndex::build_reusing_symbols` needs to decide "does this
    /// file's current content still match what was last persisted" (via
    /// `hash`), and, more cheaply first, "does it even look touched" (via
    /// `size_bytes`/`modified_at`) — without re-parsing it to find out.
    pub fn load_all_files(&self) -> Result<HashMap<FileId, FileMetadata>, IndexStoreError> {
        let conn = self.lock()?;
        let mut stmt =
            conn.prepare("SELECT file_id, file_hash, size_bytes, mtime_secs, language FROM files")?;
        let rows = stmt.query_map([], |row| {
            let file_id: String = row.get(0)?;
            let hash_bytes: Vec<u8> = row.get(1)?;
            let size_bytes: i64 = row.get(2)?;
            let mtime_secs: i64 = row.get(3)?;
            let language = str_to_language(&row.get::<_, String>(4)?).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(4, "language".into(), rusqlite::types::Type::Text)
            })?;
            Ok((file_id, hash_bytes, size_bytes, mtime_secs, language))
        })?;

        let mut out = HashMap::new();
        for row in rows {
            let (file_id, hash_bytes, size_bytes, mtime_secs, language) = row?;
            let metadata = FileMetadata {
                relative_path: file_id.clone(),
                size_bytes: size_bytes as u64,
                modified_at: UNIX_EPOCH + std::time::Duration::from_secs(mtime_secs.max(0) as u64),
                hash: hash_from_bytes(&hash_bytes),
                language,
            };
            out.insert(FileId(file_id), metadata);
        }
        Ok(out)
    }
}

/// Schema changes that `CREATE TABLE IF NOT EXISTS` cannot apply to a
/// database that already exists. Kept additive on purpose: the alternative
/// — bumping `CHUNK_VERSION` so `index_store_ensure` wipes and rebuilds —
/// would also throw away every embedding, i.e. make an existing user pay a
/// full re-embed for a search-tier improvement.
fn migrate(conn: &Connection) -> Result<(), IndexStoreError> {
    let has_fts_rowid = conn
        .prepare("SELECT 1 FROM pragma_table_info('chunks') WHERE name = 'fts_rowid'")?
        .exists([])?;
    if !has_fts_rowid {
        conn.execute_batch("ALTER TABLE chunks ADD COLUMN fts_rowid INTEGER")?;
    }
    Ok(())
}

/// Drops `file_id`'s rows from `chunks_fts`, located through `chunks` (and
/// so through `idx_chunks_file_id`) rather than by scanning the full-text
/// index for a column it does not index.
fn purge_fts_rows(tx: &rusqlite::Transaction<'_>, file_id: &FileId) -> Result<(), IndexStoreError> {
    let rowids: Vec<i64> = {
        let mut stmt = tx.prepare(
            "SELECT fts_rowid FROM chunks WHERE file_id = ?1 AND fts_rowid IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![file_id.0], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for rowid in rowids {
        tx.execute("DELETE FROM chunks_fts WHERE rowid = ?1", params![rowid])?;
    }
    Ok(())
}

/// Inserts one chunk into the full-text index, returning the rowid the
/// caller stores in `chunks.fts_rowid` to find this row again.
fn insert_fts_row(
    tx: &rusqlite::Transaction<'_>,
    chunk_id: &ChunkId,
    qualified_name: Option<&str>,
    text: &str,
) -> Result<i64, IndexStoreError> {
    tx.execute(
        "INSERT INTO chunks_fts (chunk_id, qualified_name, text) VALUES (?1, ?2, ?3)",
        params![chunk_id.0, qualified_name.unwrap_or(""), text],
    )?;
    Ok(tx.last_insert_rowid())
}

fn hash_from_bytes(bytes: &[u8]) -> blake3::Hash {
    let arr: [u8; 32] = bytes.try_into().expect("hash column is always 32 bytes");
    blake3::Hash::from(arr)
}

fn language_to_str(language: Language) -> &'static str {
    match language {
        Language::Java => "java",
        Language::Json => "json",
        Language::Yaml => "yaml",
        Language::Markdown => "markdown",
        Language::AsciiDoc => "asciidoc",
    }
}

fn str_to_language(s: &str) -> Option<Language> {
    match s {
        "java" => Some(Language::Java),
        "json" => Some(Language::Json),
        "yaml" => Some(Language::Yaml),
        "markdown" => Some(Language::Markdown),
        "asciidoc" => Some(Language::AsciiDoc),
        _ => None,
    }
}

fn chunk_kind_to_str(kind: ChunkKind) -> &'static str {
    match kind {
        ChunkKind::Method => "method",
        ChunkKind::Field => "field",
        ChunkKind::Section => "section",
        ChunkKind::File => "file",
    }
}

fn str_to_chunk_kind(s: &str) -> Option<ChunkKind> {
    match s {
        "method" => Some(ChunkKind::Method),
        "field" => Some(ChunkKind::Field),
        "section" => Some(ChunkKind::Section),
        "file" => Some(ChunkKind::File),
        _ => None,
    }
}

fn symbol_kind_to_str(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class => "class",
        SymbolKind::Interface => "interface",
        SymbolKind::Enum => "enum",
        SymbolKind::Method => "method",
        SymbolKind::Field => "field",
        SymbolKind::Section => "section",
    }
}

fn str_to_symbol_kind(s: &str) -> Option<SymbolKind> {
    match s {
        "class" => Some(SymbolKind::Class),
        "interface" => Some(SymbolKind::Interface),
        "enum" => Some(SymbolKind::Enum),
        "method" => Some(SymbolKind::Method),
        "field" => Some(SymbolKind::Field),
        "section" => Some(SymbolKind::Section),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::chunk_index::chunk_hash as compute_chunk_hash;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::SystemTime;

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("alfa-atlas-index-store-{nanos}-{n}"))
    }

    fn sample_file(file_id: &str, file_hash: blake3::Hash) -> FileMetadata {
        FileMetadata {
            relative_path: file_id.to_string(),
            size_bytes: 10,
            modified_at: SystemTime::now(),
            hash: file_hash,
            language: Language::Json,
        }
    }

    fn sample_chunk(file_id: &str, start: u32, end: u32) -> Chunk {
        sample_chunk_with_text(file_id, start, end, "{}", None)
    }

    fn sample_chunk_with_text(
        file_id: &str,
        start: u32,
        end: u32,
        text: &str,
        qualified_name: Option<&str>,
    ) -> Chunk {
        let file_hash = blake3::hash(file_id.as_bytes());
        Chunk {
            metadata: ChunkMetadata {
                id: ChunkId(format!("{file_id}#{start}-{end}")),
                file_id: FileId(file_id.to_string()),
                language: Language::Json,
                kind: ChunkKind::File,
                start_byte: start,
                end_byte: end,
                file_hash,
                hash: compute_chunk_hash(file_hash, start, end),
                qualified_name: qualified_name.map(str::to_string),
                ordinal: 0,
            },
            text: text.to_string(),
        }
    }

    #[test]
    fn meta_round_trips() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();
        assert_eq!(store.read_meta("schema_version").unwrap(), None);

        store.write_meta("schema_version", "1").unwrap();
        assert_eq!(store.read_meta("schema_version").unwrap().as_deref(), Some("1"));

        store.write_meta("schema_version", "2").unwrap();
        assert_eq!(store.read_meta("schema_version").unwrap().as_deref(), Some("2"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn chunks_round_trip_through_replace_and_load_all() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        let file_id = FileId("a.json".to_string());
        let chunks = vec![sample_chunk("a.json", 0, 10)];
        store
            .upsert_files(&[sample_file("a.json", chunks[0].metadata.file_hash)])
            .unwrap();
        store.replace_chunks_for_file(&file_id, &chunks).unwrap();

        let loaded = store.load_all_chunks().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, chunks[0].metadata.id);
        assert_eq!(loaded[0].file_hash, chunks[0].metadata.file_hash);
        assert_eq!(loaded[0].hash, chunks[0].metadata.hash);

        // Replacing again with an empty set drops the file's chunks.
        store.replace_chunks_for_file(&file_id, &[]).unwrap();
        assert!(store.load_all_chunks().unwrap().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deleting_a_chunk_cascades_to_its_embedding_row() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        let file_id = FileId("a.json".to_string());
        let chunks = vec![sample_chunk("a.json", 0, 10)];
        store
            .upsert_files(&[sample_file("a.json", chunks[0].metadata.file_hash)])
            .unwrap();
        store.replace_chunks_for_file(&file_id, &chunks).unwrap();
        store
            .upsert_embedding(&chunks[0].metadata.id, chunks[0].metadata.hash)
            .unwrap();
        assert_eq!(store.load_all_embedding_hashes().unwrap().len(), 1);

        store.replace_chunks_for_file(&file_id, &[]).unwrap();
        assert!(store.load_all_embedding_hashes().unwrap().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wipe_clears_everything() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();
        store.write_meta("k", "v").unwrap();
        let file_id = FileId("a.json".to_string());
        let chunks = vec![sample_chunk("a.json", 0, 10)];
        store
            .upsert_files(&[sample_file("a.json", chunks[0].metadata.file_hash)])
            .unwrap();
        store.replace_chunks_for_file(&file_id, &chunks).unwrap();
        store
            .upsert_embedding(&chunks[0].metadata.id, chunks[0].metadata.hash)
            .unwrap();

        store.wipe().unwrap();

        assert_eq!(store.read_meta("k").unwrap(), None);
        assert!(store.load_all_chunks().unwrap().is_empty());
        assert!(store.load_all_embedding_hashes().unwrap().is_empty());
        // `chunks_fts` is a virtual table: no foreign key reaches it, so a
        // wipe that forgot it would leave the search tier answering from
        // chunks that no longer exist.
        assert!(store.search_bm25("\"уведом\"*", 10).unwrap().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Indexes `chunks` under `file_id` after registering the file row the
    /// `chunks.file_id` foreign key requires.
    fn index_chunks(store: &IndexStore, file_id: &str, chunks: &[Chunk]) {
        store
            .upsert_files(&[sample_file(file_id, chunks[0].metadata.file_hash)])
            .unwrap();
        store
            .replace_chunks_for_file(&FileId(file_id.to_string()), chunks)
            .unwrap();
    }

    #[test]
    fn bm25_ranks_a_denser_match_above_a_passing_mention() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        index_chunks(
            &store,
            "about.json",
            &[sample_chunk_with_text(
                "about.json",
                0,
                10,
                "Порядок подачи уведомления и сроки рассмотрения уведомления.",
                None,
            )],
        );
        index_chunks(
            &store,
            "aside.json",
            &[sample_chunk_with_text(
                "aside.json",
                0,
                10,
                "Общие положения. Здесь уведомления не рассматриваются, см. другой раздел про сроки.",
                None,
            )],
        );

        let hits = store.search_bm25("\"уведом\"* OR \"подачи\"*", 10).unwrap();

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0.0, "about.json#0-10");
        assert!(
            hits[0].1 > hits[1].1,
            "score must be higher-is-better after the bm25() flip: {hits:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bm25_matches_a_russian_query_across_case_and_inflection() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        index_chunks(
            &store,
            "a.json",
            &[sample_chunk_with_text(
                "a.json",
                0,
                10,
                "УВЕДОМЛЕНИЙ по заявке",
                None,
            )],
        );

        // Query said `уведомления`, the text says `УВЕДОМЛЕНИЙ`: `unicode61`
        // folds the case, and the stem prefix `fts5_query` cut it down to
        // bridges the inflection the full word would have missed.
        let hits = store.search_bm25("\"уведом\"*", 10).unwrap();

        assert_eq!(hits.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn qualified_name_is_searchable_alongside_the_text() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        index_chunks(
            &store,
            "a.json",
            &[sample_chunk_with_text(
                "a.json",
                0,
                10,
                "нет ни одного совпадения в тексте",
                Some("UserService.getNotifications"),
            )],
        );

        let hits = store.search_bm25("\"getnotifications\"*", 10).unwrap();

        assert_eq!(hits.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn re_chunking_a_file_drops_its_stale_fts_rows() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        index_chunks(
            &store,
            "a.json",
            &[sample_chunk_with_text("a.json", 0, 10, "первоначальный текст", None)],
        );
        assert_eq!(store.search_bm25("\"первон\"*", 10).unwrap().len(), 1);

        index_chunks(
            &store,
            "a.json",
            &[sample_chunk_with_text("a.json", 0, 10, "переписанный текст", None)],
        );

        assert!(store.search_bm25("\"первон\"*", 10).unwrap().is_empty());
        assert_eq!(store.search_bm25("\"перепи\"*", 10).unwrap().len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deleting_a_file_purges_its_fts_rows() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        index_chunks(
            &store,
            "a.json",
            &[sample_chunk_with_text("a.json", 0, 10, "исчезающий текст", None)],
        );
        store.delete_files(&[FileId("a.json".to_string())]).unwrap();

        assert!(store.search_bm25("\"исчеза\"*", 10).unwrap().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn backfilling_fts_leaves_the_embeddings_it_would_be_ruinous_to_recompute() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        let file_id = FileId("a.json".to_string());
        let chunk = sample_chunk_with_text("a.json", 0, 10, "текст чанка", None);
        index_chunks(&store, "a.json", std::slice::from_ref(&chunk));
        store
            .upsert_embedding(&chunk.metadata.id, chunk.metadata.hash)
            .unwrap();

        store
            .replace_fts_for_file(
                &file_id,
                &[(chunk.metadata.id.clone(), None, "текст чанка".to_string())],
            )
            .unwrap();

        // The whole reason this is a separate write path from
        // `replace_chunks_for_file`: that one cascades embeddings away.
        assert_eq!(store.load_all_embedding_hashes().unwrap().len(), 1);
        assert_eq!(store.load_all_chunks().unwrap().len(), 1);
        // Exactly one row, not the original plus a duplicate.
        assert_eq!(store.search_bm25("\"чанка\"*", 10).unwrap().len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fts_version_gates_the_backfill_until_it_is_marked_current() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        assert!(store.fts_needs_backfill().unwrap());
        store.mark_fts_current().unwrap();
        assert!(!store.fts_needs_backfill().unwrap());

        // A wipe clears `meta` too, so a rebuilt store asks again.
        store.wipe().unwrap();
        assert!(store.fts_needs_backfill().unwrap());

        std::fs::remove_dir_all(&dir).ok();
    }

    fn sample_symbol(name: &str, start_byte: u32, end_byte: u32) -> Symbol {
        Symbol {
            name: name.to_string(),
            kind: SymbolKind::Class,
            start_line: 0,
            end_line: 1,
            start_byte,
            end_byte,
        }
    }

    #[test]
    fn symbols_round_trip_through_replace_and_load_all() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        let file_id = FileId("a.json".to_string());
        store
            .upsert_files(&[sample_file("a.json", blake3::hash(b"x"))])
            .unwrap();
        let symbols = vec![sample_symbol("UserService", 0, 10)];
        store.replace_symbols_for_file(&file_id, &symbols).unwrap();

        let loaded = store.load_all_symbols().unwrap();
        assert_eq!(loaded.get(&file_id).unwrap().len(), 1);
        assert_eq!(loaded.get(&file_id).unwrap()[0].name, "UserService");
        assert_eq!(loaded.get(&file_id).unwrap()[0].kind, SymbolKind::Class);

        // Replacing again with an empty set drops the file's symbols.
        store.replace_symbols_for_file(&file_id, &[]).unwrap();
        assert!(!store.load_all_symbols().unwrap().contains_key(&file_id));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn imports_round_trip_through_replace_and_load_all() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        let file_id = FileId("com/foo/Bar.java".to_string());
        store
            .upsert_files(&[sample_file("com/foo/Bar.java", blake3::hash(b"x"))])
            .unwrap();
        let imports = vec![
            ImportRef { fqn: "com.foo.Baz".to_string(), is_wildcard: false },
            ImportRef { fqn: "com.foo.util".to_string(), is_wildcard: true },
        ];
        store.replace_imports_for_file(&file_id, &imports).unwrap();

        let loaded = store.load_all_imports().unwrap();
        let loaded_imports = loaded.get(&file_id).unwrap();
        assert_eq!(loaded_imports.len(), 2);
        assert!(loaded_imports.contains(&ImportRef { fqn: "com.foo.Baz".to_string(), is_wildcard: false }));
        assert!(loaded_imports.contains(&ImportRef { fqn: "com.foo.util".to_string(), is_wildcard: true }));

        // Replacing again with an empty set drops the file's imports.
        store.replace_imports_for_file(&file_id, &[]).unwrap();
        assert!(!store.load_all_imports().unwrap().contains_key(&file_id));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deleting_a_file_cascades_to_its_symbol_rows() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        let file_id = FileId("a.json".to_string());
        store
            .upsert_files(&[sample_file("a.json", blake3::hash(b"x"))])
            .unwrap();
        store
            .replace_symbols_for_file(&file_id, &[sample_symbol("Foo", 0, 3)])
            .unwrap();
        assert_eq!(store.load_all_symbols().unwrap().len(), 1);

        store.delete_files(&[file_id]).unwrap();
        assert!(store.load_all_symbols().unwrap().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_all_files_reflects_upserted_files() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();

        let hash = blake3::hash(b"content");
        store.upsert_files(&[sample_file("a.json", hash)]).unwrap();

        let files = store.load_all_files().unwrap();
        let metadata = files.get(&FileId("a.json".to_string())).unwrap();
        assert_eq!(metadata.hash, hash);
        assert_eq!(metadata.size_bytes, 10);
        assert_eq!(metadata.language, Language::Json);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wipe_clears_symbols_too() {
        let dir = fixture_dir();
        let store = IndexStore::open(&dir).unwrap();
        let file_id = FileId("a.json".to_string());
        store
            .upsert_files(&[sample_file("a.json", blake3::hash(b"x"))])
            .unwrap();
        store
            .replace_symbols_for_file(&file_id, &[sample_symbol("Foo", 0, 3)])
            .unwrap();
        store
            .replace_imports_for_file(&file_id, &[ImportRef { fqn: "com.foo.Bar".to_string(), is_wildcard: false }])
            .unwrap();

        store.wipe().unwrap();
        assert!(store.load_all_symbols().unwrap().is_empty());
        assert!(store.load_all_imports().unwrap().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reopening_the_same_dir_preserves_data() {
        let dir = fixture_dir();
        {
            let store = IndexStore::open(&dir).unwrap();
            store.write_meta("k", "v").unwrap();
        }
        let reopened = IndexStore::open(&dir).unwrap();
        assert_eq!(reopened.read_meta("k").unwrap().as_deref(), Some("v"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
