use crate::model::{Device, Event};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionDescriptor {
    pub version: u32,
    pub id: String,
    pub directory: PathBuf,
    pub device: Option<Device>,
    pub started_at: String,
    pub legacy: bool,
    pub metadata: serde_json::Value,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionCatalog {
    pub sessions: Vec<SessionDescriptor>,
    pub scanned: u32,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineOrder {
    OldestFirst,
    #[default]
    NewestFirst,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineQuery {
    pub source: Option<String>,
    pub search: String,
    pub order: TimelineOrder,
    pub limit: u32,
    pub cursor: Option<String>,
    pub after_sequence: Option<u64>,
    pub through_sequence: Option<u64>,
}

impl Default for TimelineQuery {
    fn default() -> Self {
        Self {
            source: None,
            search: String::new(),
            order: TimelineOrder::NewestFirst,
            limit: 200,
            cursor: None,
            after_sequence: None,
            through_sequence: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimelineRow {
    pub sequence: u64,
    pub event: Event,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimelinePage {
    pub generation: String,
    pub rows: Vec<TimelineRow>,
    pub next_cursor: Option<String>,
    pub total: u64,
    pub matching: u64,
    pub progress: IndexProgress,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexProgress {
    pub indexed: u64,
    pub raw_bytes: u64,
    pub indexed_bytes: u64,
    pub backlog_bytes: u64,
    pub complete: bool,
    pub latest_sequence: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventDetail {
    pub sequence: u64,
    pub event: Event,
    pub detail_bytes: u64,
    pub detail_complete: bool,
    pub bodies: Vec<BodyDescriptor>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataChunk {
    pub offset: u64,
    pub total_bytes: u64,
    pub data_base64: String,
    pub next_offset: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BodySide {
    Request,
    Response,
}

impl BodySide {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Request => "request_body",
            Self::Response => "response_body",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BodyDescriptor {
    pub side: BodySide,
    pub available: bool,
    pub reason: Option<String>,
    pub bytes: u64,
    pub text: bool,
    pub byte_basis: String,
    pub content_encoding: Option<String>,
    pub decoded_available: bool,
    pub decoded_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BodyChunk {
    pub body: BodyDescriptor,
    pub chunk: DataChunk,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RawTarget {
    pub path: PathBuf,
    pub relative_path: String,
    pub line: u64,
}
