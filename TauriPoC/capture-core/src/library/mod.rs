mod bodies;
mod index;
mod normalize;
mod query;

pub use query::*;

use crate::{
    files,
    lease::FileLease,
    model::{Event, Result},
    timeline,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use index::Index;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const METADATA_BOUND: u64 = 256 * 1024;
const DETAIL_BOUND: usize = 256 * 1024;
const CHUNK_BOUND: u32 = 64 * 1024;

pub struct EvidenceLibrary {
    descriptor: SessionDescriptor,
    index: Index,
    live: bool,
    _cache_writer: FileLease,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    generation: String,
    query: String,
    timestamp: String,
    rank: u8,
    line: u64,
    id: String,
}

impl EvidenceLibrary {
    pub fn open(directory: PathBuf, cache_directory: PathBuf, live: bool) -> Result<Self> {
        let descriptor = inspect(&directory)?;
        let evidence = fs::canonicalize(&directory).map_err(|e| e.to_string())?;
        let proposed_cache = canonical_destination(&cache_directory)?;
        if proposed_cache.starts_with(&evidence) {
            return Err("Disposable cache must be outside evidence".into());
        }
        crate::lease::private_directory(&cache_directory)?;
        let cache_directory = fs::canonicalize(cache_directory).map_err(|e| e.to_string())?;
        if cache_directory.starts_with(&evidence) {
            return Err("Disposable cache must be outside evidence".into());
        }
        let cache_id = timeline::hash(evidence.to_string_lossy().as_bytes());
        let writer = FileLease::acquire(&cache_directory.join(format!("{cache_id}.writer")))
            .map_err(|e| format!("Evidence cache already has a writer or is unsafe: {e}"))?;
        let index = Index::open(&cache_directory.join(format!("{cache_id}.sqlite")))?;
        let mut library = Self {
            descriptor,
            index,
            live,
            _cache_writer: writer,
        };
        library.refresh()?;
        Ok(library)
    }

    pub fn catalog(output_root: &Path, limit: u32) -> Result<SessionCatalog> {
        if !(1..=200).contains(&limit) {
            return Err("Catalog limit must be between 1 and 200".into());
        }
        refuse_directory_link(output_root)?;
        let mut catalog = SessionCatalog {
            sessions: vec![],
            scanned: 0,
            truncated: false,
            warnings: vec![],
        };
        for entry in fs::read_dir(output_root).map_err(|e| e.to_string())? {
            if catalog.scanned == 2048 {
                catalog.truncated = true;
                break;
            }
            catalog.scanned += 1;
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if !kind.is_dir() || kind.is_symlink() {
                continue;
            }
            match inspect(&entry.path()) {
                Ok(descriptor) => {
                    catalog.sessions.push(descriptor);
                    catalog
                        .sessions
                        .sort_by(|a, b| (&b.started_at, &a.id).cmp(&(&a.started_at, &b.id)));
                    if catalog.sessions.len() > limit as usize {
                        catalog.truncated = true;
                        catalog.sessions.truncate(limit as usize);
                    }
                }
                Err(error) => {
                    if catalog.warnings.len() < 32 {
                        catalog
                            .warnings
                            .push(format!("{}: {error}", entry.file_name().to_string_lossy()));
                    }
                }
            }
        }
        Ok(catalog)
    }

    pub fn descriptor(&self) -> &SessionDescriptor {
        &self.descriptor
    }

    pub fn set_live(&mut self, live: bool) {
        self.live = live;
    }

    pub fn refresh(&mut self) -> Result<IndexProgress> {
        self.index
            .refresh(&self.descriptor.directory, &self.descriptor, self.live)
    }

    pub fn query(&mut self, query: TimelineQuery) -> Result<TimelinePage> {
        if !(1..=2000).contains(&query.limit) {
            return Err("Timeline limit must be between 1 and 2000".into());
        }
        if query.search.len() > 4096 {
            return Err("Timeline search exceeds 4096 UTF-8 bytes".into());
        }
        if query
            .source
            .as_deref()
            .is_some_and(|s| !matches!(s, "logs" | "network" | "marker"))
        {
            return Err("Unknown timeline source".into());
        }
        for sequence in [query.after_sequence, query.through_sequence]
            .into_iter()
            .flatten()
        {
            if sequence > i64::MAX as u64 {
                return Err("Sequence is outside the supported range".into());
            }
        }
        self.index.validate(&self.descriptor.directory)?;
        let mut identity = query.clone();
        identity.cursor = None;
        let query_id = timeline::hash(&serde_json::to_vec(&identity).map_err(|e| e.to_string())?);
        let cursor = query
            .cursor
            .as_deref()
            .map(|encoded| -> Result<Cursor> {
                if encoded.len() > 2048 {
                    return Err("Invalid timeline cursor".into());
                }
                let bytes = STANDARD
                    .decode(encoded)
                    .map_err(|_| "Invalid timeline cursor")?;
                let cursor: Cursor =
                    serde_json::from_slice(&bytes).map_err(|_| "Invalid timeline cursor")?;
                if cursor.generation != self.index.generation
                    || cursor.query != query_id
                    || cursor.line > i64::MAX as u64
                    || cursor.rank > 2
                    || cursor.id.len() > 128
                    || cursor.timestamp.len() > 64
                {
                    return Err("Timeline cursor is stale or belongs to another query".into());
                }
                Ok(cursor)
            })
            .transpose()?;
        let search = normalize::fold(&query.search);
        let after = query.after_sequence.unwrap_or(0);
        let through = query.through_sequence.unwrap_or(i64::MAX as u64);
        let filter = "(?1 IS NULL OR source=?1) AND (?2='' OR instr(search,?2)>0) AND sequence>?3 AND sequence<=?4";
        let matching: u64 = self
            .index
            .db
            .query_row(
                &format!("SELECT count(*) FROM events WHERE {filter}"),
                params![query.source, search, after as i64, through as i64],
                |r| sql_u64(r, 0),
            )
            .map_err(|e| e.to_string())?;
        let ascending = query.order == TimelineOrder::OldestFirst;
        let direction = if ascending { "ASC" } else { "DESC" };
        let comparison = if ascending { ">" } else { "<" };
        let sql = format!("SELECT sequence,preview,timestamp,source_rank,line,id FROM events WHERE {filter}
            AND (?5=0 OR (timestamp,source_rank,line,id) {comparison} (?6,?7,?8,?9))
            ORDER BY timestamp {direction},source_rank {direction},line {direction},id {direction} LIMIT ?10");
        let mut statement = self.index.db.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = statement
            .query_map(
                params![
                    query.source,
                    search,
                    after as i64,
                    through as i64,
                    u8::from(cursor.is_some()),
                    cursor.as_ref().map(|c| c.timestamp.as_str()).unwrap_or(""),
                    cursor.as_ref().map(|c| c.rank).unwrap_or(0),
                    cursor.as_ref().map(|c| c.line as i64).unwrap_or(0),
                    cursor.as_ref().map(|c| c.id.as_str()).unwrap_or(""),
                    query.limit + 1
                ],
                |row| {
                    Ok((
                        sql_u64(row, 0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u8>(3)?,
                        sql_u64(row, 4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .map_err(|e| e.to_string())?;
        let mut values = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let has_more = values.len() > query.limit as usize;
        values.truncate(query.limit as usize);
        let next_cursor = if has_more {
            values
                .last()
                .map(|(_, _, timestamp, rank, line, id)| {
                    serde_json::to_vec(&Cursor {
                        generation: self.index.generation.clone(),
                        query: query_id,
                        timestamp: timestamp.clone(),
                        rank: *rank,
                        line: *line,
                        id: id.clone(),
                    })
                    .map(|bytes| STANDARD.encode(bytes))
                    .map_err(|e| e.to_string())
                })
                .transpose()?
        } else {
            None
        };
        let rows = values
            .into_iter()
            .map(|(sequence, preview, _, _, _, _)| {
                Ok(TimelineRow {
                    sequence,
                    event: serde_json::from_str(&preview).map_err(|e| e.to_string())?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let progress = self.index.progress()?;
        Ok(TimelinePage {
            generation: self.index.generation.clone(),
            rows,
            next_cursor,
            total: progress.indexed,
            matching,
            progress,
        })
    }

    pub fn event(&mut self, id: &str) -> Result<Option<EventDetail>> {
        let Some((sequence, mut event)) = self.full_event(id)? else {
            return Ok(None);
        };
        let detail_bytes = serde_json::to_vec(&event.detail)
            .map_err(|e| e.to_string())?
            .len() as u64;
        let bodies = [BodySide::Request, BodySide::Response]
            .into_iter()
            .filter(|side| event.detail.get(side.key()).is_some())
            .map(|side| bodies::view(&event, side, false).0)
            .collect();
        for key in ["request_body", "response_body"] {
            if let Some(value) = event.detail.get_mut(key).and_then(Value::as_object_mut) {
                value.remove("text");
            }
        }
        let detail_complete =
            serde_json::to_vec(&event).map_err(|e| e.to_string())?.len() <= DETAIL_BOUND;
        if !detail_complete {
            timeline::preview(&mut event);
        }
        Ok(Some(EventDetail {
            sequence,
            event,
            detail_bytes,
            detail_complete,
            bodies,
        }))
    }

    pub fn detail_chunk(&mut self, id: &str, offset: u64, limit: u32) -> Result<DataChunk> {
        let (_, event) = self
            .full_event(id)?
            .ok_or("Timeline event is no longer indexed")?;
        let bytes = serde_json::to_vec(&event.detail).map_err(|e| e.to_string())?;
        chunk(&bytes, offset, limit)
    }

    pub fn body_chunk(
        &mut self,
        id: &str,
        side: BodySide,
        offset: u64,
        limit: u32,
    ) -> Result<BodyChunk> {
        let (_, event) = self
            .full_event(id)?
            .ok_or("Timeline event is no longer indexed")?;
        let (body, bytes) = bodies::view(&event, side, false);
        let chunk = chunk(&bytes, offset, limit)?;
        Ok(BodyChunk { body, chunk })
    }

    pub fn decoded_body_chunk(
        &mut self,
        id: &str,
        side: BodySide,
        offset: u64,
        limit: u32,
    ) -> Result<BodyChunk> {
        let (_, event) = self
            .full_event(id)?
            .ok_or("Timeline event is no longer indexed")?;
        let (body, bytes) = bodies::view(&event, side, true);
        let chunk = chunk(&bytes, offset, limit)?;
        Ok(BodyChunk { body, chunk })
    }

    pub fn raw_target(&mut self, id: &str) -> Result<RawTarget> {
        let (_, event) = self
            .full_event(id)?
            .ok_or("Timeline event is no longer indexed")?;
        let (relative_path, line) = event
            .raw_ref
            .rsplit_once("#L")
            .ok_or("Invalid raw evidence reference")?;
        if !index::SOURCES
            .iter()
            .any(|(_, path)| *path == relative_path)
        {
            return Err("Raw reference is outside evidence allowlist".into());
        }
        let file =
            files::open(&self.descriptor.directory, relative_path).map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Raw target is not a regular file".into());
        }
        Ok(RawTarget {
            path: self.descriptor.directory.join(relative_path),
            relative_path: relative_path.into(),
            line: line.parse().map_err(|_| "Invalid raw reference line")?,
        })
    }

    fn full_event(&mut self, id: &str) -> Result<Option<(u64, Event)>> {
        if id.len() > 128 {
            return Err("Invalid timeline event identifier".into());
        }
        self.index.validate(&self.descriptor.directory)?;
        let record = self
            .index
            .db
            .query_row(
                "SELECT sequence,source,path,line,offset,length,raw_hash FROM events WHERE id=?1",
                [id],
                |r| {
                    Ok((
                        sql_u64(r, 0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        sql_u64(r, 3)?,
                        sql_u64(r, 4)?,
                        sql_u64(r, 5)?,
                        r.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some((sequence, source, path, line, offset, length, hash)) = record else {
            return Ok(None);
        };
        if length > index::RECORD_BOUND as u64 {
            return Err("Cached evidence record exceeds the raw record bound".into());
        }
        if !index::SOURCES
            .iter()
            .any(|(s, p)| *s == source && *p == path)
        {
            return Err("Cached evidence reference is outside allowlist".into());
        }
        let mut file = files::open(&self.descriptor.directory, &path).map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Evidence is not a regular file".into());
        }
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        file.take(length)
            .read_to_end(&mut raw)
            .map_err(|e| e.to_string())?;
        if raw.len() as u64 != length || timeline::hash(&raw) != hash {
            self.index.reset()?;
            return Err("Raw evidence changed; timeline cache was invalidated".into());
        }
        Ok(Some((
            sequence,
            normalize::event(&source, &path, line, &raw, &self.descriptor.started_at),
        )))
    }
}

fn chunk(bytes: &[u8], offset: u64, limit: u32) -> Result<DataChunk> {
    if !(1..=CHUNK_BOUND).contains(&limit) {
        return Err("Chunk limit must be between 1 and 65536 bytes".into());
    }
    let start = usize::try_from(offset).map_err(|_| "Chunk offset is outside evidence")?;
    if start > bytes.len() {
        return Err("Chunk offset is outside evidence".into());
    }
    let end = start.saturating_add(limit as usize).min(bytes.len());
    Ok(DataChunk {
        offset,
        total_bytes: bytes.len() as u64,
        data_base64: STANDARD.encode(&bytes[start..end]),
        next_offset: (end < bytes.len()).then_some(end as u64),
    })
}

fn inspect(directory: &Path) -> Result<SessionDescriptor> {
    refuse_directory_link(directory)?;
    let (filename, legacy) = if index::open_optional(directory, "capture.json")?.is_some() {
        ("capture.json", false)
    } else {
        ("session.json", true)
    };
    let mut file = files::open(directory, filename)
        .map_err(|e| format!("Session metadata is unavailable: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Session metadata is not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(METADATA_BOUND + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > METADATA_BOUND {
        return Err("Session metadata exceeds 256 KiB".into());
    }
    let metadata: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid session metadata: {e}"))?;
    let version = if legacy {
        0
    } else {
        metadata["version"]
            .as_u64()
            .ok_or("Missing public descriptor version")?
    };
    if !legacy && version != 1 {
        return Err("Unsupported public session descriptor version".into());
    }
    let id = metadata["id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or("Missing or invalid session identifier")?
        .to_owned();
    let started_at = metadata["started_at"]
        .as_str()
        .filter(|s| s.len() <= 64 && chrono::DateTime::parse_from_rfc3339(s).is_ok())
        .ok_or("Missing or invalid session start time")?
        .to_owned();
    let device = metadata
        .get("device")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| format!("Invalid session device metadata: {e}"))?;
    let keys = [
        "version",
        "id",
        "device",
        "started_at",
        "selection",
        "log_scope",
        "limits",
        "network_policy",
        "fixture",
        "context",
        "log_attribution",
        "phase",
        "completion",
        "routing_restored",
        "interrupted",
        "error",
        "coordinator",
    ];
    let public = keys
        .into_iter()
        .filter_map(|key| metadata.get(key).map(|v| (key.into(), v.clone())))
        .collect::<serde_json::Map<_, _>>();
    Ok(SessionDescriptor {
        version: version as u32,
        id,
        directory: directory.to_path_buf(),
        device,
        started_at,
        legacy,
        metadata: Value::Object(public),
        warnings: if legacy {
            vec!["Legacy metadata may omit the device, capture policy, or context".into()]
        } else {
            vec![]
        },
    })
}

fn refuse_directory_link(path: &Path) -> Result<()> {
    let info = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !info.is_dir() || info.file_type().is_symlink() {
        return Err("Session folder is not a real directory".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if info.file_attributes() & 0x400 != 0 {
            return Err("Reparse session folder refused".into());
        }
    }
    Ok(())
}

fn canonical_destination(path: &Path) -> Result<PathBuf> {
    let mut existing = path.to_path_buf();
    let mut missing = Vec::new();
    while !existing.exists() {
        missing.push(
            existing
                .file_name()
                .ok_or("Invalid cache directory")?
                .to_owned(),
        );
        existing = existing
            .parent()
            .ok_or("Invalid cache directory")?
            .to_path_buf();
    }
    let mut destination = fs::canonicalize(existing).map_err(|e| e.to_string())?;
    for component in missing.into_iter().rev() {
        destination.push(component);
    }
    Ok(destination)
}

#[cfg(test)]
mod tests;

fn sql_u64(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(e),
        )
    })
}
