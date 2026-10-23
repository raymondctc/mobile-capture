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
            bytes: 200_000_000,
            records: 250_000,
            duration_ms: 600_000,
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
            return Err("Limits must be positive and no larger than 200 MB, 250,000 records, and 600 seconds".into());
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
    IdentityGap,
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
    #[serde(default)]
    pub log_scope: LogScope,
    #[serde(default)]
    pub resolved_logs: Option<crate::android::ResolvedLogs>,
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
    #[serde(default)]
    pub descriptor_version: u32,
    #[serde(default)]
    pub selection: CaptureSelection,
    #[serde(default)]
    pub log_scope: LogScope,
    #[serde(default)]
    pub network_policy: NetworkPolicy,
    #[serde(default)]
    pub context: CaptureContext,
    #[serde(default)]
    pub log_attribution: Option<crate::android::LogAttribution>,
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
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "mode", content = "consent", rename_all = "snake_case")]
pub enum CaptureSelection {
    #[default]
    Logs,
    Network(NetworkConsent),
    LogsAndNetwork(NetworkConsent),
}
impl CaptureSelection {
    pub fn includes_logs(&self) -> bool {
        !matches!(self, Self::Network(_))
    }
    pub fn includes_network(&self) -> bool {
        !matches!(self, Self::Logs)
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum LogScope {
    App {
        application_id: String,
    },
    #[default]
    WholeDevice,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CaptureContext {
    pub title: String,
    pub notes: String,
    pub steps: Vec<String>,
}
impl CaptureContext {
    pub fn validate(&self) -> Result<()> {
        if self.title.len() > 512
            || self.notes.len() > 32_768
            || self.steps.len() > 100
            || self.steps.iter().any(|s| s.len() > 4096)
        {
            return Err("Capture context exceeds title, notes, or reproduction step limits".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkPolicy {
    pub max_flows: u64,
    pub max_body_bytes: u64,
    pub max_header_bytes: u64,
    pub max_total_bytes: u64,
    pub duration_ms: u64,
}
impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            max_flows: 10_000,
            max_body_bytes: 32 * 1024 * 1024,
            max_header_bytes: 128 * 1024,
            max_total_bytes: 1024 * 1024 * 1024,
            duration_ms: 3_600_000,
        }
    }
}
impl NetworkPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.max_flows == 0
            || self.max_flows > 1_000_000
            || self.max_body_bytes == 0
            || self.max_body_bytes > 32 * 1024 * 1024
            || self.max_header_bytes == 0
            || self.max_header_bytes > 128 * 1024
            || self.max_total_bytes < 1024 * 1024
            || self.max_total_bytes > 1024 * 1024 * 1024
            || self.duration_ms == 0
            || self.duration_ms > 86_400_000
        {
            return Err("Network policy requires at least 1 MiB total, other values positive and within 1,000,000 flows, 32 MiB/body, 128 KiB headers, 1 GiB total, and 24 hours".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionDescriptor {
    pub version: u32,
    pub id: String,
    pub device: Device,
    pub started_at: String,
    pub selection: CaptureSelection,
    pub log_scope: LogScope,
    pub limits: Limits,
    pub network_policy: NetworkPolicy,
    pub fixture: bool,
    pub context: CaptureContext,
    pub log_attribution: Option<crate::android::LogAttribution>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartRequest {
    pub serial: String,
    pub whole_device_consent: bool,
    pub selection: CaptureSelection,
    pub limits: Limits,
    #[serde(default)]
    pub log_scope: LogScope,
    #[serde(default)]
    pub network_policy: NetworkPolicy,
    #[serde(default)]
    pub context: CaptureContext,
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
