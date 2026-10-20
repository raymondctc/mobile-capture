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
    #[serde(default)]
    pub source_lease: bool,
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
#[derive(Debug, Serialize, Deserialize)]
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
    pub network: NetworkView,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "phase", content = "session", rename_all = "snake_case")]
pub enum Snapshot {
    Idle { fixture: bool, limits: Limits },
    Recording(SessionView),
    Stopped(SessionView),
    Failed(SessionView),
    UnverifiedShutdown(SessionView),
    RecoveryRequired(SessionView),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapturePaths {
    pub output_root: PathBuf,
    pub runtime_root: Option<PathBuf>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSelection {
    pub adb: PathBuf,
    pub worker: PathBuf,
    pub guardian: PathBuf,
    pub mitmdump: Option<PathBuf>,
    pub addon: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkConsent {
    pub whole_device: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", content = "consent", rename_all = "snake_case")]
pub enum CaptureSelection {
    Logs,
    LogsAndNetwork(NetworkConsent),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartRequest {
    pub serial: String,
    pub whole_device_consent: bool,
    pub selection: CaptureSelection,
    pub limits: Limits,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CoverageView {
    pub decoded_http_events: u64,
    pub tls_errors: u64,
    pub omitted_flows: u64,
    pub unavailable_bodies: u64,
    pub admission_closed: bool,
    pub public_certificate: Option<PathBuf>,
    pub synthetic_check_url: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryProblem {
    pub message: String,
    pub guardian_interrupted: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkCompletion {
    pub coverage: CoverageView,
    pub routing_restored: bool,
    pub partial: bool,
    pub forced: bool,
    pub interrupted: bool,
    pub reasons: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum NetworkView {
    NotSelected,
    Preparing,
    Recording(CoverageView),
    RecoveryRequired(RecoveryProblem),
    Stopped(NetworkCompletion),
    Interrupted(NetworkCompletion),
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "session", rename_all = "snake_case")]
pub enum StopOutcome {
    Ready(SessionView),
    RecoveryRequired(SessionView),
    Unverified(SessionView),
}
