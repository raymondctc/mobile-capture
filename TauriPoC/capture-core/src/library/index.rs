use super::{normalize, IndexProgress, SessionDescriptor};
use crate::{files, model::Result, timeline};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, Metadata},
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(super) const SOURCES: [(&str, &str); 3] = [
    ("logs", "logs/device.log"),
    ("network", "network/events.jsonl"),
    ("marker", "timeline/markers.jsonl"),
];
pub(super) const RECORD_BOUND: usize = 128 * 1024 * 1024;
const READ_BUDGET: usize = 8 * 1024 * 1024 / 3;
const RECORD_BUDGET: usize = 2_000 / 3;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Source {
    pub path: String,
    pub identity: String,
    pub offset: u64,
    pub line: u64,
    pub observed_len: u64,
    modified: String,
    checkpoint: String,
    terminal_tail: bool,
    #[serde(skip)]
    pending: Vec<u8>,
    #[serde(skip)]
    scanned_pending: usize,
}

pub(super) struct Index {
    pub db: Connection,
    pub generation: String,
    sources: Vec<Source>,
    raw_bytes: u64,
    caught_up: bool,
}

struct Record {
    source: &'static str,
    path: &'static str,
    line: u64,
    offset: u64,
    bytes: Vec<u8>,
}

impl Index {
    pub fn open(path: &Path) -> Result<Self> {
        files::refuse_link(path)?;
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        db.busy_timeout(std::time::Duration::from_millis(250))
            .map_err(|e| e.to_string())?;
        db.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS events (
               sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL,
               timestamp TEXT NOT NULL, source_rank INTEGER NOT NULL, line INTEGER NOT NULL,
               source TEXT NOT NULL, path TEXT NOT NULL, offset INTEGER NOT NULL,
               length INTEGER NOT NULL, raw_hash TEXT NOT NULL, preview TEXT NOT NULL,
               search TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS timeline_order ON events(timestamp,source_rank,line,id);
             CREATE INDEX IF NOT EXISTS timeline_source ON events(source,sequence);",
        )
        .map_err(|e| e.to_string())?;
        let version: Option<String> = db
            .query_row("SELECT value FROM meta WHERE key='version'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        if version.as_deref() != Some("1") {
            db.execute_batch("DELETE FROM events; DELETE FROM meta;")
                .map_err(|e| e.to_string())?;
            db.execute("INSERT INTO meta VALUES ('version','1')", [])
                .map_err(|e| e.to_string())?;
        }
        let generation = db
            .query_row("SELECT value FROM meta WHERE key='generation'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(generation);
        db.execute(
            "INSERT OR REPLACE INTO meta VALUES ('generation',?1)",
            [&generation],
        )
        .map_err(|e| e.to_string())?;
        let saved: Option<String> = db
            .query_row("SELECT value FROM meta WHERE key='sources'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        let sources: Vec<Source> = saved
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let sources = if sources.len() == SOURCES.len()
            && sources.iter().zip(SOURCES).all(|(s, (_, p))| s.path == p)
        {
            sources
        } else {
            db.execute("DELETE FROM events", [])
                .map_err(|e| e.to_string())?;
            fresh_sources()
        };
        Ok(Self {
            db,
            generation,
            sources,
            raw_bytes: 0,
            caught_up: false,
        })
    }

    pub fn reset(&mut self) -> Result<()> {
        self.generation = generation();
        self.db
            .execute_batch("DELETE FROM events; DELETE FROM sqlite_sequence WHERE name='events';")
            .map_err(|e| e.to_string())?;
        self.db
            .execute(
                "INSERT OR REPLACE INTO meta VALUES ('generation',?1)",
                [&self.generation],
            )
            .map_err(|e| e.to_string())?;
        self.db
            .execute("DELETE FROM meta WHERE key='sources'", [])
            .map_err(|e| e.to_string())?;
        self.sources = fresh_sources();
        Ok(())
    }

    pub fn validate(&mut self, root: &Path) -> Result<bool> {
        for source in &self.sources {
            match open_optional(root, &source.path)? {
                None if !source.identity.is_empty() => {
                    self.reset()?;
                    return Ok(false);
                }
                None => {}
                Some(mut file) => {
                    let metadata = file.metadata().map_err(|e| e.to_string())?;
                    if !metadata.is_file() {
                        return Err("Evidence is not a regular file".into());
                    }
                    if !source.identity.is_empty()
                        && (identity(&file, &metadata)? != source.identity
                            || metadata.len() < source.offset + source.pending.len() as u64
                            || (source.terminal_tail && metadata.len() != source.observed_len)
                            || (metadata.len() <= source.observed_len
                                && modified(&metadata) != source.modified)
                            || checkpoint(&mut file, source.offset)? != source.checkpoint)
                    {
                        self.reset()?;
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    pub fn refresh(
        &mut self,
        root: &Path,
        descriptor: &SessionDescriptor,
        live: bool,
    ) -> Result<IndexProgress> {
        self.validate(root)?;
        let mut sources = self.sources.clone();
        let mut records = Vec::new();
        let mut raw_bytes = 0;
        let mut caught_up = true;
        for (state, (source, path)) in sources.iter_mut().zip(SOURCES) {
            let Some(mut file) = open_optional(root, path)? else {
                continue;
            };
            let metadata = file.metadata().map_err(|e| e.to_string())?;
            if !metadata.is_file() {
                return Err(format!("Evidence is not a regular file: {path}"));
            }
            raw_bytes += metadata.len();
            state.identity = identity(&file, &metadata)?;
            let read_offset = state.offset + state.pending.len() as u64;
            file.seek(SeekFrom::Start(read_offset))
                .map_err(|e| e.to_string())?;
            let read = file
                .by_ref()
                .take(READ_BUDGET as u64)
                .read_to_end(&mut state.pending)
                .map_err(|e| e.to_string())?;
            let at_end = read_offset + read as u64 == metadata.len();
            let mut start = 0;
            let mut position = state.scanned_pending;
            let mut count = 0;
            while position < state.pending.len() && count < RECORD_BUDGET {
                let byte = state.pending[position];
                if byte == b'\n' || byte == b'\r' {
                    if byte == b'\r' && position + 1 == state.pending.len() && (live || !at_end) {
                        break;
                    }
                    append_record(
                        &mut records,
                        source,
                        path,
                        state.line + 1,
                        state.offset + start as u64,
                        &state.pending[start..position],
                    )?;
                    position += 1;
                    if byte == b'\r' && state.pending.get(position) == Some(&b'\n') {
                        position += 1;
                    }
                    start = position;
                    state.line += 1;
                    count += 1;
                } else {
                    position += 1;
                    if position - start > RECORD_BOUND {
                        return Err(format!("Evidence record exceeds 128 MiB: {path}"));
                    }
                }
            }
            if !live
                && at_end
                && position == state.pending.len()
                && start < position
                && count < RECORD_BUDGET
            {
                append_record(
                    &mut records,
                    source,
                    path,
                    state.line + 1,
                    state.offset + start as u64,
                    &state.pending[start..position],
                )?;
                state.line += 1;
                start = position;
                state.terminal_tail = true;
            }
            state.pending.drain(..start);
            state.scanned_pending = position - start;
            state.offset += start as u64;
            if state.pending.len() > RECORD_BOUND {
                return Err(format!("Evidence record exceeds 128 MiB: {path}"));
            }
            state.observed_len = metadata.len();
            state.modified = modified(&metadata);
            state.checkpoint = checkpoint(&mut file, state.offset)?;
            caught_up &= at_end && (state.pending.is_empty() || (live && count < RECORD_BUDGET));
        }
        let transaction = self.db.transaction().map_err(|e| e.to_string())?;
        for record in records {
            let mut event = normalize::event(
                record.source,
                record.path,
                record.line,
                &record.bytes,
                &descriptor.started_at,
            );
            let search = normalize::searchable(&event, &record.bytes, &descriptor.metadata);
            timeline::preview(&mut event);
            let preview = serde_json::to_string(&event).map_err(|e| e.to_string())?;
            transaction.execute(
                "INSERT INTO events (id,timestamp,source_rank,line,source,path,offset,length,raw_hash,preview,search)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![event.id, event.timestamp, source_rank(record.source), record.line as i64, record.source,
                    record.path, record.offset as i64, record.bytes.len() as i64, timeline::hash(&record.bytes), preview, search]
            ).map_err(|e| e.to_string())?;
        }
        let state_json = serde_json::to_string(&sources).map_err(|e| e.to_string())?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO meta VALUES ('sources',?1)",
                [state_json],
            )
            .map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())?;
        self.sources = sources;
        self.raw_bytes = raw_bytes;
        self.caught_up = caught_up;
        self.progress()
    }

    pub fn progress(&self) -> Result<IndexProgress> {
        let (indexed, latest_sequence) = self
            .db
            .query_row(
                "SELECT count(*),coalesce(max(sequence),0) FROM events",
                [],
                |r| Ok((super::sql_u64(r, 0)?, super::sql_u64(r, 1)?)),
            )
            .map_err(|e| e.to_string())?;
        let indexed_bytes = self.sources.iter().map(|s| s.offset).sum();
        Ok(IndexProgress {
            indexed,
            raw_bytes: self.raw_bytes,
            indexed_bytes,
            backlog_bytes: self.raw_bytes.saturating_sub(indexed_bytes),
            complete: self.caught_up && indexed_bytes == self.raw_bytes,
            latest_sequence,
        })
    }
}

fn fresh_sources() -> Vec<Source> {
    SOURCES
        .into_iter()
        .map(|(_, path)| Source {
            path: path.into(),
            ..Source::default()
        })
        .collect()
}

fn append_record(
    records: &mut Vec<Record>,
    source: &'static str,
    path: &'static str,
    line: u64,
    offset: u64,
    bytes: &[u8],
) -> Result<()> {
    if bytes.len() > RECORD_BOUND {
        return Err(format!("Evidence record exceeds 128 MiB: {path}"));
    }
    if !bytes.iter().all(u8::is_ascii_whitespace) {
        records.push(Record {
            source,
            path,
            line,
            offset,
            bytes: bytes.to_vec(),
        });
    }
    Ok(())
}

pub(super) fn source_rank(source: &str) -> u8 {
    match source {
        "logs" => 0,
        "network" => 1,
        _ => 2,
    }
}

pub(super) fn open_optional(root: &Path, path: &str) -> Result<Option<File>> {
    match files::open(root, path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Refused evidence path {path}: {error}")),
    }
}

fn checkpoint(file: &mut File, offset: u64) -> Result<String> {
    let mut bytes = Vec::new();
    for start in [0, offset.saturating_sub(4096)] {
        file.seek(SeekFrom::Start(start))
            .map_err(|e| e.to_string())?;
        file.take((offset - start).min(4096))
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
    }
    Ok(timeline::hash(&bytes))
}

fn modified(metadata: &Metadata) -> String {
    format!("{:?}", metadata.modified().ok())
}

#[cfg(unix)]
fn identity(_file: &File, metadata: &Metadata) -> Result<String> {
    use std::os::unix::fs::MetadataExt;
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn identity(file: &File, _metadata: &Metadata) -> Result<String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(format!(
        "{}:{}:{}",
        information.dwVolumeSerialNumber, information.nFileIndexHigh, information.nFileIndexLow
    ))
}

fn generation() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS random source required for cache generations");
    timeline::hash(&bytes)[..32].into()
}
