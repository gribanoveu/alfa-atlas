//! On-disk OptMem store — fixed-width LOG.txt + TREE/ summaries.
//!
//! Mirrors VictorTaelin/OptMem's Python store: position *is* identity,
//! append-only log, rebuildable tree cache. No background work.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use thiserror::Error;

use crate::domain::optmem::{
    check_entry, pad_record, parse_log_line, MemoryEntry, OptMemError, OptMemKnobs, LOG_REC,
    TREE_REC,
};

#[derive(Debug, Error)]
pub enum OptMemStoreError {
    #[error("{0}")]
    Domain(#[from] OptMemError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("the summary of #{lo}-{hi} is corrupt")]
    CorruptSummary { lo: usize, hi: usize },
    #[error("the summary of #{lo}-{hi} is blank")]
    BlankSummary { lo: usize, hi: usize },
    #[error("log index {0} is out of range")]
    LogIndexOutOfRange(usize),
}

pub struct OptMemStore {
    dir: PathBuf,
    knobs: OptMemKnobs,
}

impl OptMemStore {
    /// Create the store directory (and empty LOG/TREE/config) if missing.
    pub fn init(dir: &Path) -> Result<Self, OptMemStoreError> {
        fs::create_dir_all(dir.join("TREE"))?;
        let log = dir.join("LOG.txt");
        if !log.exists() {
            File::create(&log)?;
        }
        let config_path = dir.join("config");
        if !config_path.exists() {
            write_config(dir, &BTreeMap::new())?;
        }
        Self::open(dir)
    }

    /// Open an existing store (creates TREE/ if needed). Refuses a missing
    /// directory — callers that want auto-create use `init`.
    pub fn open(dir: &Path) -> Result<Self, OptMemStoreError> {
        if !dir.is_dir() {
            return Err(OptMemStoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("No memory at {}", dir.display()),
            )));
        }
        fs::create_dir_all(dir.join("TREE"))?;
        let log = dir.join("LOG.txt");
        if !log.exists() {
            File::create(&log)?;
        }
        let overrides = read_overrides(dir)?;
        let knobs = OptMemKnobs::with_overrides(&overrides)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            knobs,
        })
    }

    /// Open if present, else `init`.
    pub fn open_or_init(dir: &Path) -> Result<Self, OptMemStoreError> {
        if dir.is_dir() {
            Self::open(dir)
        } else {
            Self::init(dir)
        }
    }

    /// Open an existing store directory, or `Ok(None)` if it does not exist.
    /// Never creates files — use for read paths (`wake` inject, `recall`) so
    /// merely chatting does not materialize empty memory trees on disk.
    pub fn open_if_exists(dir: &Path) -> Result<Option<Self>, OptMemStoreError> {
        if dir.is_dir() {
            Self::open(dir).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn knobs(&self) -> OptMemKnobs {
        self.knobs
    }

    pub fn log_len(&self) -> Result<usize, OptMemStoreError> {
        count_records(&self.log_path(), LOG_REC)
    }

    pub fn level_len(&self, size: usize) -> Result<usize, OptMemStoreError> {
        count_records(&self.tree_path(size), TREE_REC)
    }

    pub fn log_get(&self, i: usize) -> Result<MemoryEntry, OptMemStoreError> {
        let mut slice = self.log_slice(i, i + 1)?;
        slice
            .pop()
            .ok_or_else(|| OptMemStoreError::Io(std::io::Error::other("empty log slice")))
    }

    pub fn log_slice(&self, lo: usize, hi: usize) -> Result<Vec<MemoryEntry>, OptMemStoreError> {
        if hi <= lo {
            return Ok(Vec::new());
        }
        let mut f = File::open(self.log_path())?;
        f.seek(SeekFrom::Start((lo * LOG_REC) as u64))?;
        let mut buf = vec![0u8; (hi - lo) * LOG_REC];
        let n = f.read(&mut buf)?;
        buf.truncate(n - n % LOG_REC);
        Ok(decode_log_records(&buf))
    }

    /// Stream every memory — used by `recall`.
    pub fn log_scan(&self) -> Result<LogScan, OptMemStoreError> {
        Ok(LogScan {
            file: File::open(self.log_path())?,
            buf: Vec::new(),
            pos: 0,
        })
    }

    pub fn tree_get(&self, lo: usize, hi: usize) -> Result<Option<String>, OptMemStoreError> {
        let size = hi - lo;
        let path = self.tree_path(size);
        let mut f = match File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        f.seek(SeekFrom::Start(((lo / size) * TREE_REC) as u64))?;
        let mut rec = vec![0u8; TREE_REC];
        let n = f.read(&mut rec)?;
        if n < TREE_REC {
            return Ok(None);
        }
        match String::from_utf8(rec) {
            Ok(s) => {
                let s = s.trim_end().to_string();
                if s.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(s))
                }
            }
            Err(_) => Err(OptMemStoreError::CorruptSummary {
                lo,
                hi: hi - 1,
            }),
        }
    }

    /// Append memories. Returns the first id used.
    pub fn log_append(&self, items: &[(String, String)]) -> Result<usize, OptMemStoreError> {
        let _lock = self.lock()?;
        repair(&self.log_path(), LOG_REC)?;
        let base = count_records(&self.log_path(), LOG_REC)?;
        let mut f = OpenOptions::new().append(true).open(self.log_path())?;
        for (k, (date, text)) in items.iter().enumerate() {
            let line = format!("#{} {} {}", base + k, date, text);
            let rec = pad_record(&line, LOG_REC)?;
            f.write_all(&rec)?;
        }
        f.flush()?;
        f.sync_all()?;
        Ok(base)
    }

    /// Write block `[lo, hi)`. Returns false if a parallel writer already
    /// advanced this level past `lo`.
    pub fn tree_put(&self, lo: usize, hi: usize, text: &str) -> Result<bool, OptMemStoreError> {
        let _lock = self.lock()?;
        let size = hi - lo;
        let path = self.tree_path(size);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !path.exists() {
            File::create(&path)?;
        }
        repair(&path, TREE_REC)?;
        if count_records(&path, TREE_REC)? != lo / size {
            return Ok(false);
        }
        let mut f = OpenOptions::new().append(true).open(&path)?;
        let rec = pad_record(text, TREE_REC)?;
        f.write_all(&rec)?;
        f.flush()?;
        f.sync_all()?;
        Ok(true)
    }

    /// Remove one raw log entry by index and rebuild LOG.txt. OptMem ids are
    /// positional — remaining entries are renumbered `#0..`. TREE summaries
    /// are cleared because block ranges no longer match the log.
    pub fn log_remove_at(&self, index: usize) -> Result<(), OptMemStoreError> {
        let _lock = self.lock()?;
        repair(&self.log_path(), LOG_REC)?;
        let mut entries: Vec<MemoryEntry> = self
            .log_scan()?
            .collect::<Result<Vec<_>, _>>()?;
        if index >= entries.len() {
            return Err(OptMemStoreError::LogIndexOutOfRange(index));
        }
        entries.remove(index);
        let path = self.log_path();
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        for (k, entry) in entries.iter().enumerate() {
            let line = format!("#{} {} {}", k, entry.date, entry.text);
            let rec = pad_record(&line, LOG_REC)?;
            f.write_all(&rec)?;
        }
        f.flush()?;
        f.sync_all()?;
        clear_tree_files(&self.dir)?;
        Ok(())
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join("LOG.txt")
    }

    fn tree_path(&self, size: usize) -> PathBuf {
        self.dir.join("TREE").join(size.to_string())
    }

    fn lock(&self) -> Result<File, OptMemStoreError> {
        let lock_path = self.dir.join(".lock");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(lock_path)?;
        file.lock_exclusive()?;
        Ok(file)
    }
}

pub struct LogScan {
    file: File,
    buf: Vec<u8>,
    pos: usize,
}

impl Iterator for LogScan {
    type Item = Result<MemoryEntry, OptMemStoreError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.pos + LOG_REC <= self.buf.len() {
                let slice = &self.buf[self.pos..self.pos + LOG_REC];
                self.pos += LOG_REC;
                let text = String::from_utf8_lossy(slice);
                return Some(
                    parse_log_line(&text)
                        .ok_or_else(|| {
                            OptMemStoreError::Io(std::io::Error::other("corrupt log record"))
                        }),
                );
            }
            self.buf.clear();
            self.pos = 0;
            let mut chunk = vec![0u8; LOG_REC * 4096];
            match self.file.read(&mut chunk) {
                Ok(0) => return None,
                Ok(n) => {
                    chunk.truncate(n - n % LOG_REC);
                    if chunk.is_empty() {
                        return None;
                    }
                    self.buf = chunk;
                }
                Err(e) => return Some(Err(e.into())),
            }
        }
    }
}

fn clear_tree_files(dir: &Path) -> Result<(), OptMemStoreError> {
    let tree = dir.join("TREE");
    if !tree.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&tree)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn count_records(path: &Path, rec: usize) -> Result<usize, OptMemStoreError> {
    match fs::metadata(path) {
        Ok(m) => Ok((m.len() as usize) / rec),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e.into()),
    }
}

fn repair(path: &Path, rec: usize) -> Result<(), OptMemStoreError> {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let n = meta.len() as usize;
    if !n.is_multiple_of(rec) {
        let f = OpenOptions::new().write(true).open(path)?;
        f.set_len((n - n % rec) as u64)?;
    }
    Ok(())
}

fn decode_log_records(buf: &[u8]) -> Vec<MemoryEntry> {
    let mut out = Vec::new();
    for i in 0..(buf.len() / LOG_REC) {
        let slice = &buf[i * LOG_REC..(i + 1) * LOG_REC];
        let text = String::from_utf8_lossy(slice);
        if let Some(e) = parse_log_line(&text) {
            out.push(e);
        }
    }
    out
}

fn read_overrides(dir: &Path) -> Result<BTreeMap<String, usize>, OptMemStoreError> {
    let path = dir.join("config");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out = BTreeMap::new();
    for (n, line) in content.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() || !line.contains('=') {
            continue;
        }
        let (k, v) = line.split_once('=').unwrap();
        let k = k.trim().to_uppercase();
        let v = v.trim();
        if !crate::domain::optmem::KNOB_NAMES.contains(&k.as_str()) {
            return Err(OptMemError::UnknownKnob(k).into());
        }
        let parsed: usize = v.parse().map_err(|_| OptMemError::InvalidKnobValue {
            name: k.clone(),
            value: v.to_string(),
        })?;
        let _ = OptMemKnobs::validate_positive(&k, parsed).map_err(|e| {
            OptMemStoreError::Domain(match e {
                OptMemError::InvalidKnobValue { name, value } => OptMemError::InvalidKnobValue {
                    name: format!("{} line {}: {}", path.display(), n + 1, name),
                    value,
                },
                other => other,
            })
        })?;
        if k == "ENTRY_CHARS" {
            let top = (TREE_REC - 8).min(LOG_REC - 40);
            if parsed > top {
                return Err(OptMemError::EntryCharsTooLarge { max: top }.into());
            }
        }
        crate::domain::optmem::check_knob_max(&k, parsed)?;
        out.insert(k, parsed);
    }
    Ok(out)
}

fn write_config(dir: &Path, over: &BTreeMap<String, usize>) -> Result<(), OptMemStoreError> {
    let defaults = OptMemKnobs::default();
    let mut out = String::from(
        "# OptMem sizes for this memory. A commented line means: follow the\n\
         # tool's default. Edit with memory config NAME=VALUE.\n\n",
    );
    let knobs = [
        ("WAKE_LINES", defaults.wake_lines, "the memory context: how many lines wake prints"),
        ("ENTRY_CHARS", defaults.entry_chars, "the longest one memory may be, in bytes"),
        ("PART_CHARS", defaults.part_chars, "output paging: largest part, in bytes"),
        ("PART_LINES", defaults.part_lines, "output paging: largest part, in lines"),
    ];
    for (name, default, what) in knobs {
        if let Some(v) = over.get(name) {
            out.push_str(&format!("{name:<12} = {v:<6} # {what}\n"));
        } else {
            out.push_str(&format!("# {name:<12} = {default:<6} # {what}\n"));
        }
    }
    fs::write(dir.join("config"), out)?;
    Ok(())
}

/// Helper used by service layer: validate entry text against store knobs.
pub fn validate_entry(store: &OptMemStore, text: &str) -> Result<String, OptMemStoreError> {
    Ok(check_entry(text, store.knobs().entry_chars)?)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Several tests in this module call this concurrently. A nanosecond
    /// timestamp alone does not reliably disambiguate them on a coarser
    /// system clock — two would share a directory and clobber each other.
    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("optmem-store-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn append_wake_nap_recall_forget_round_trip() {
        let dir = temp_dir();
        let store = OptMemStore::init(&dir).unwrap();

        let base = store
            .log_append(&[
                ("2026-08-11".into(), "first fact".into()),
                ("2026-08-11".into(), "second fact".into()),
            ])
            .unwrap();
        assert_eq!(base, 0);
        assert_eq!(store.log_len().unwrap(), 2);

        let e0 = store.log_get(0).unwrap();
        assert_eq!(e0.text, "first fact");

        assert!(store.tree_put(0, 2, "both facts").unwrap());
        assert_eq!(store.tree_get(0, 2).unwrap().as_deref(), Some("both facts"));

        let hits: Vec<_> = store
            .log_scan()
            .unwrap()
            .filter_map(|r| r.ok())
            .filter(|e| e.text.contains("second"))
            .collect();
        assert_eq!(hits.len(), 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_or_init_creates_missing_dir() {
        let dir = temp_dir().join("nested");
        OptMemStore::open_or_init(&dir).unwrap();
        assert!(dir.join("LOG.txt").exists());
        fs::remove_dir_all(dir.parent().unwrap()).ok();
    }

    #[test]
    fn open_if_exists_returns_none_without_creating() {
        let dir = temp_dir().join("absent");
        assert!(OptMemStore::open_if_exists(&dir).unwrap().is_none());
        assert!(!dir.exists());
    }

    #[test]
    fn open_rejects_a_hand_edited_config_above_the_wake_lines_cap() {
        use crate::domain::optmem::WAKE_LINES_MAX;

        let dir = temp_dir();
        fs::write(
            dir.join("config"),
            format!("WAKE_LINES = {}\n", WAKE_LINES_MAX + 1),
        )
        .unwrap();

        let result = OptMemStore::open(&dir);
        assert!(matches!(
            result,
            Err(OptMemStoreError::Domain(OptMemError::KnobTooLarge { .. }))
        ));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn log_remove_at_renumbers_and_clears_tree() {
        let dir = temp_dir();
        let store = OptMemStore::init(&dir).unwrap();
        store
            .log_append(&[
                ("2026-08-11".into(), "keep".into()),
                ("2026-08-11".into(), "drop".into()),
                ("2026-08-11".into(), "also keep".into()),
            ])
            .unwrap();
        assert!(store.tree_put(0, 2, "summary").unwrap());
        store.log_remove_at(1).unwrap();
        assert_eq!(store.log_len().unwrap(), 2);
        assert_eq!(store.log_get(0).unwrap().text, "keep");
        assert_eq!(store.log_get(1).unwrap().text, "also keep");
        assert!(store.tree_get(0, 2).unwrap().is_none());
        fs::remove_dir_all(&dir).ok();
    }
}
