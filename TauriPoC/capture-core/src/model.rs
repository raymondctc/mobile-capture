use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, String>;
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    Ready,
    Unauthorized,
    Offline,
    Unavailable,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub serial: String,
    pub name: String,
    pub state: DeviceState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Limits {
    pub bytes: u64,
    pub records: u64,
    pub duration_ms: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 32 * 1024 * 1024,
            records: 100_000,
            duration_ms: 120_000,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        let max = Self::default();
        if self.bytes == 0
            || self.bytes > max.bytes
            || self.records == 0
            || self.records > max.records
            || self.duration_ms == 0
            || self.duration_ms > max.duration_ms
        {
            return Err("Limits must be positive and no larger than 32 MiB, 100,000 records, and 120 seconds".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    LeaseClosed,
    DurationLimit,
    ByteLimit,
    RecordLimit,
    CollectorExit,
    ReadError,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub reason: StopReason,
    pub bytes: u64,
    pub records: u64,
    pub incomplete_tail: bool,
    pub child_reaped: bool,
    pub forced: bool,
    pub exit_code: Option<i32>,
    pub stderr: String,
    pub ended_at: String,
}
#[derive(Serialize, Deserialize)]
pub struct WorkerConfig {
    pub adb: PathBuf,
    pub serial: String,
    pub directory: PathBuf,
    pub limits: Limits,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub timestamp: String,
    pub original_timestamp: Option<String>,
    pub received_at: Option<String>,
    pub source: String,
    pub kind: String,
    pub summary: String,
    pub raw_ref: String,
    pub detail: serde_json::Value,
    pub clock_uncertainty_ms: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct SessionView {
    pub id: String,
    pub device: Device,
    pub directory: PathBuf,
    pub started_at: String,
    pub limits: Limits,
    pub fixture: bool,
    pub bytes: u64,
    pub logs: usize,
    pub markers: usize,
    pub events: Vec<Event>,
    pub receipt: Option<Receipt>,
    pub archive: Option<PathBuf>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(tag = "phase", content = "session", rename_all = "snake_case")]
pub enum Snapshot {
    Idle { fixture: bool, limits: Limits },
    Recording(SessionView),
    Stopped(SessionView),
    Failed(SessionView),
    UnverifiedShutdown(SessionView),
}
