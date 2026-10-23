use crate::{
    files,
    model::*,
    process::{self, Job},
    timeline,
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) struct LogCollector {
    child: Child,
    input: Option<ChildStdin>,
    _job: Job,
}
impl LogCollector {
    pub(crate) fn launch(worker: &Path, config: &WorkerConfig) -> Result<Self> {
        let mut command = Command::new(worker);
        process::quiet(&mut command);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Cannot launch collector worker: {e}"))?;
        let job = match Job::contain(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let mut input = child.stdin.take().ok_or("Worker stdin unavailable")?;
        let data = serde_json::to_vec(config).map_err(|e| e.to_string())?;
        if let Err(error) = input
            .write_all(&data)
            .and_then(|_| input.write_all(b"\n"))
            .and_then(|_| input.flush())
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.to_string());
        }
        let mut collector = Self {
            child,
            input: Some(input),
            _job: job,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while config.source_lease
            && !crate::lease::read::<bool>(&config.directory.join("runtime/log-ready.json"))
                .unwrap_or(false)
        {
            if collector
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_some()
                || Instant::now() >= deadline
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(collector)
    }
    pub(crate) fn close(&mut self) -> Result<()> {
        self.input.take();
        match process::wait_bounded(&mut self.child, Duration::from_secs(4)) {
            Ok(_) => Ok(()),
            Err(error) => {
                let _ = self.child.kill();
                let _ = self.child.wait();
                Err(error)
            }
        }
    }
}
impl Drop for LogCollector {
    fn drop(&mut self) {
        if self.close().is_err() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

enum Lifecycle {
    Recording(LogCollector),
    Finished(Receipt),
    Unverified(String),
}
struct Session {
    id: String,
    device: Device,
    directory: PathBuf,
    started_at: String,
    limits: Limits,
    lifecycle: Lifecycle,
    archive: Option<PathBuf>,
    markers: std::fs::File,
    descriptor: SessionDescriptor,
    quiescent: Option<crate::lease::FileLease>,
}
pub(crate) struct LogSession {
    adb: PathBuf,
    worker: PathBuf,
    root: PathBuf,
    fixture: bool,
    session: Option<Session>,
}
impl LogSession {
    pub fn new(adb: PathBuf, worker: PathBuf, root: PathBuf, fixture: bool) -> Self {
        Self {
            adb,
            worker,
            root,
            fixture,
            session: None,
        }
    }
    pub fn start(
        &mut self,
        request: StartRequest,
        resolved: crate::android::ResolvedLogs,
    ) -> Result<Snapshot> {
        let serial = request.serial.clone();
        let limits = request.limits.clone();
        limits.validate()?;
        self.refresh()?;
        if self.session.as_ref().is_some_and(|s| {
            matches!(
                s.lifecycle,
                Lifecycle::Recording(_) | Lifecycle::Unverified(_)
            )
        }) {
            return Err(
                "Stop and verify the existing collector before starting another capture".into(),
            );
        }
        let device = crate::android::devices(&self.adb)?
            .into_iter()
            .find(|d| d.serial == serial && d.state == DeviceState::Ready)
            .ok_or("Select an authorized, ready Android device")?;
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let directory = tempfile::Builder::new()
            .prefix("tauri-poc-")
            .tempdir_in(&self.root)
            .map_err(|e| e.to_string())?
            .keep();
        fs::create_dir(directory.join("timeline")).map_err(|e| e.to_string())?;
        crate::lease::private_directory(&directory.join("runtime"))?;
        let markers = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("timeline/markers.jsonl"))
            .map_err(|e| e.to_string())?;
        let id = directory
            .file_name()
            .ok_or("Missing session directory name")?
            .to_string_lossy()
            .into_owned();
        let started_at = now();
        fs::write(directory.join("session.json"), serde_json::to_vec(&serde_json::json!({"id": id, "started_at": started_at, "coordinator": "tauri-poc"})).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let descriptor = SessionDescriptor {
            version: 1,
            id: id.clone(),
            device: device.clone(),
            started_at: started_at.clone(),
            selection: request.selection.clone(),
            log_scope: request.log_scope.clone(),
            limits: limits.clone(),
            network_policy: request.network_policy.clone(),
            fixture: self.fixture,
            context: request.context.clone(),
            log_attribution: Some(resolved.attribution.clone()),
        };
        crate::lease::write(&directory.join("capture.json"), &descriptor)?;
        let lease = LogCollector::launch(
            &self.worker,
            &WorkerConfig {
                adb: self.adb.clone(),
                serial,
                directory: directory.clone(),
                limits: limits.clone(),
                source_lease: true,
                log_scope: request.log_scope,
                resolved_logs: Some(resolved),
            },
        )?;
        self.session = Some(Session {
            id,
            device,
            directory,
            started_at,
            limits,
            lifecycle: Lifecycle::Recording(lease),
            archive: None,
            markers,
            descriptor,
            quiescent: None,
        });
        self.snapshot()
    }
    fn refresh(&mut self) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        if let Lifecycle::Recording(lease) = &mut session.lifecycle {
            match lease.child.try_wait().map_err(|e| e.to_string())? {
                None => {}
                Some(status) => {
                    let receipt = match files::read(&session.directory, "receipt.json") {
                        Ok(bytes) => {
                            bytes.and_then(|bytes| serde_json::from_slice::<Receipt>(&bytes).ok())
                        }
                        Err(error) => {
                            session.lifecycle = Lifecycle::Unverified(error);
                            return Ok(());
                        }
                    };
                    session.lifecycle = match receipt {
                        Some(receipt) if status.success() && receipt.child_reaped => Lifecycle::Finished(receipt),
                        _ => Lifecycle::Unverified(format!("Worker exited with {status} without a verified collector receipt. Export is disabled.")),
                    };
                }
            }
        }
        if let Lifecycle::Finished(_) = &session.lifecycle {
            if session.quiescent.is_none() {
                session.markers.sync_all().map_err(|e| e.to_string())?;
                session.quiescent = Some(crate::lease::FileLease::acquire(
                    &session.directory.join("runtime/logs.writer"),
                )?);
                let proof = log_proof(&session.directory)?;
                crate::lease::write(
                    &session.directory.join("runtime/log-completion.json"),
                    &proof,
                )?;
            }
        }
        Ok(())
    }
    pub fn open_saved(&mut self, directory: PathBuf) -> Result<Snapshot> {
        self.refresh()?;
        if self.session.as_ref().is_some_and(|s| {
            matches!(
                s.lifecycle,
                Lifecycle::Recording(_) | Lifecycle::Unverified(_)
            )
        }) {
            return Err(
                "Stop and verify the active collector before opening saved evidence".into(),
            );
        }
        let directory = fs::canonicalize(directory).map_err(|e| e.to_string())?;
        let descriptor: SessionDescriptor = serde_json::from_slice(&files::read(&directory,"capture.json")?.ok_or("Saved logs have no versioned descriptor. Legacy evidence can be browsed without control admission")?).map_err(|e| e.to_string())?;
        if descriptor.version != 1 || !matches!(descriptor.selection, CaptureSelection::Logs) {
            return Err("Saved session is not a supported log-only capture".into());
        }
        let lease = crate::lease::FileLease::acquire(&directory.join("runtime/logs.writer"))?;
        verify_log_proof(&directory)?;
        let receipt: Receipt = serde_json::from_slice(
            &files::read(&directory, "receipt.json")?.ok_or("Saved collector receipt missing")?,
        )
        .map_err(|e| e.to_string())?;
        if !receipt.child_reaped {
            return Err("Saved collector shutdown was not verified".into());
        }
        let markers =
            files::open(&directory, "timeline/markers.jsonl").map_err(|e| e.to_string())?;
        self.session = Some(Session {
            id: descriptor.id.clone(),
            device: descriptor.device.clone(),
            directory,
            started_at: descriptor.started_at.clone(),
            limits: descriptor.limits.clone(),
            lifecycle: Lifecycle::Finished(receipt),
            archive: None,
            markers,
            descriptor,
            quiescent: Some(lease),
        });
        self.snapshot()
    }
    pub fn status(&mut self) -> Result<Snapshot> {
        self.snapshot_impl(false)
    }
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        self.snapshot_impl(true)
    }
    fn snapshot_impl(&mut self, preview: bool) -> Result<Snapshot> {
        self.refresh()?;
        let Some(session) = &self.session else {
            return Ok(Snapshot::Idle {
                fixture: self.fixture,
                limits: Limits::default(),
            });
        };
        let bytes = fs::metadata(session.directory.join("logs/device.log")).map_or(0, |m| m.len());
        let logs = if preview {
            files::read(&session.directory, "logs/device.log")?.unwrap_or_default()
        } else {
            Vec::new()
        };
        let markers = if preview {
            files::read(&session.directory, "timeline/markers.jsonl")?.unwrap_or_default()
        } else {
            Vec::new()
        };
        let live_logs = if matches!(session.lifecycle, Lifecycle::Recording(_)) {
            &logs[..logs
                .iter()
                .rposition(|b| *b == b'\n')
                .map(|i| i + 1)
                .unwrap_or(0)]
        } else {
            &logs
        };
        let mut events = timeline::events(live_logs, &markers, &session.started_at);
        let log_count = events.iter().filter(|event| event.source == "logs").count();
        let marker_count = events.len() - log_count;
        timeline::retain_recent(&mut events);
        for event in &mut events {
            if event.summary.len() > 1024
                || event
                    .original_timestamp
                    .as_ref()
                    .is_some_and(|s| s.len() > 64)
            {
                timeline::preview(event);
            }
        }
        let receipt = if let Lifecycle::Finished(receipt) = &session.lifecycle {
            Some(receipt.clone())
        } else {
            None
        };
        let mut warnings = vec![
            format!("Android log scope: {:?}. All available emitted levels from main, system, and crash buffers.", session.descriptor.log_scope),
            "Device clock uncertainty is unknown. Time proximity does not establish causality."
                .into(),
        ];
        if self.fixture {
            warnings.push(
                "FIXTURE MODE. Evidence comes from the compiled fake ADB, not a phone.".into(),
            );
        }
        if let Lifecycle::Unverified(error) = &session.lifecycle {
            warnings.push(error.clone());
        }
        if let Some(receipt) = &receipt {
            if receipt.reason != StopReason::LeaseClosed {
                warnings.push(format!(
                    "Collector ended: {:?}. Review partial coverage in the manifest.",
                    receipt.reason
                ));
            }
            if !receipt.stderr.is_empty() {
                warnings.push(receipt.stderr.clone());
            }
            if receipt.forced {
                warnings.push("Collector required forced termination. Shutdown was verified, but coverage is partial.".into());
            }
            if receipt.incomplete_tail {
                warnings.push("Raw capture ends with an incomplete record.".into());
            }
        }
        let view = SessionView {
            id: session.id.clone(),
            device: session.device.clone(),
            directory: session.directory.clone(),
            started_at: session.started_at.clone(),
            limits: session.limits.clone(),
            fixture: self.fixture,
            bytes,
            logs: log_count,
            markers: marker_count,
            events,
            receipt,
            archive: session.archive.clone(),
            network: NetworkView::NotSelected,
            warnings,
            descriptor_version: session.descriptor.version,
            selection: session.descriptor.selection.clone(),
            log_scope: session.descriptor.log_scope.clone(),
            network_policy: session.descriptor.network_policy.clone(),
            context: session.descriptor.context.clone(),
            log_attribution: session.descriptor.log_attribution.clone(),
        };
        Ok(match &session.lifecycle {
            Lifecycle::Recording(_) => Snapshot::Recording(view),
            Lifecycle::Unverified(_) => Snapshot::UnverifiedShutdown(view),
            Lifecycle::Finished(receipt)
                if matches!(
                    receipt.reason,
                    StopReason::CollectorExit | StopReason::ReadError | StopReason::IdentityGap
                ) =>
            {
                Snapshot::Failed(view)
            }
            Lifecycle::Finished(_) => Snapshot::Stopped(view),
        })
    }
    pub fn marker(&mut self, text: String) -> Result<Snapshot> {
        self.refresh()?;
        let session = self
            .session
            .as_mut()
            .ok_or("Start capture before adding a marker")?;
        if !matches!(session.lifecycle, Lifecycle::Recording(_)) {
            return Err("Markers require an active capture".into());
        }
        if text.trim().is_empty() || text.len() > 4096 {
            return Err("Marker text must contain 1 to 4096 UTF-8 bytes".into());
        }
        let path = session.directory.join("timeline/markers.jsonl");
        if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) >= 1024 * 1024 {
            return Err("The 1 MiB marker limit has been reached".into());
        }
        let mut data = serde_json::to_vec(
            &serde_json::json!({"timestamp": now(), "text": text, "clock_uncertainty_ms": 1}),
        )
        .map_err(|e| e.to_string())?;
        data.push(b'\n');
        session
            .markers
            .write_all(&data)
            .and_then(|_| session.markers.sync_data())
            .map_err(|e| e.to_string())?;
        self.snapshot()
    }
    pub fn stop(&mut self) -> Result<Snapshot> {
        if let Some(session) = self.session.as_mut() {
            if let Lifecycle::Recording(lease) = &mut session.lifecycle {
                if let Err(error) = lease.close() {
                    session.lifecycle =
                        Lifecycle::Unverified(format!("Shutdown unverified: {error}"));
                }
            }
        }
        self.snapshot()
    }
    pub fn prepare_save(&mut self) -> Result<crate::save::SaveHandle> {
        self.refresh()?;
        let session = self.session.as_ref().ok_or("There is no capture to save")?;
        if !matches!(session.lifecycle, Lifecycle::Finished(_)) || session.quiescent.is_none() {
            return Err("Save requires verified collector shutdown".into());
        }
        verify_log_proof(&session.directory)?;
        crate::save::SaveHandle::create(&session.directory)
    }
    pub fn export_with_control(&mut self, control: &crate::save::SaveHandle) -> Result<PathBuf> {
        self.refresh()?;
        let session = self.session.as_ref().ok_or("There is no capture to save")?;
        control.matches(&session.directory)?;
        if !matches!(session.lifecycle, Lifecycle::Finished(_)) || session.quiescent.is_none() {
            return Err("Save requires verified collector shutdown".into());
        }
        control.run(|| self.export_impl(Some(control)))
    }
    pub fn export(&mut self) -> Result<PathBuf> {
        self.export_impl(None)
    }
    fn export_impl(&mut self, control: Option<&crate::save::SaveHandle>) -> Result<PathBuf> {
        self.refresh()?;
        let session = self
            .session
            .as_mut()
            .ok_or("There is no capture to export")?;
        let Lifecycle::Finished(receipt) = &session.lifecycle else {
            return Err("Export requires a verified stopped collector".into());
        };
        if let Some(control) = control {
            control.matches(&session.directory)?;
        }
        verify_log_proof_controlled(&session.directory, control)?;
        let archive = match control {
            Some(control) => crate::evidence::export_controlled(
                &session.directory,
                &session.id,
                &session.device,
                &session.started_at,
                &session.limits,
                receipt,
                self.fixture,
                None,
                control,
            )?,
            None => crate::evidence::export(
                &session.directory,
                &session.id,
                &session.device,
                &session.started_at,
                &session.limits,
                receipt,
                self.fixture,
            )?,
        };
        session.archive = Some(archive.clone());
        Ok(archive)
    }
    pub fn directory(&self) -> Option<&Path> {
        self.session.as_ref().map(|s| s.directory.as_path())
    }
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq)]
struct LogProof {
    version: u32,
    hashes: std::collections::BTreeMap<String, String>,
}
fn log_proof(directory: &Path) -> Result<LogProof> {
    log_proof_controlled(directory, None)
}
fn log_proof_controlled(
    directory: &Path,
    control: Option<&crate::save::SaveHandle>,
) -> Result<LogProof> {
    let mut hashes = std::collections::BTreeMap::new();
    for path in [
        "capture.json",
        "receipt.json",
        "logs/device.log",
        "timeline/markers.jsonl",
    ] {
        let bytes = crate::save::read(directory, path, control)?
            .ok_or_else(|| format!("Stopped source missing: {path}"))?;
        hashes.insert(path.to_owned(), crate::save::hash(&bytes, control)?);
    }
    Ok(LogProof { version: 1, hashes })
}
fn verify_log_proof(directory: &Path) -> Result<()> {
    verify_log_proof_controlled(directory, None)
}
fn verify_log_proof_controlled(
    directory: &Path,
    control: Option<&crate::save::SaveHandle>,
) -> Result<()> {
    let saved: LogProof = serde_json::from_slice(
        &files::read(directory, "runtime/log-completion.json")?
            .ok_or("Saved logs have no private verified completion proof")?,
    )
    .map_err(|e| e.to_string())?;
    if saved != log_proof_controlled(directory, control)? {
        return Err("Saved logs differ from verified completion proof".into());
    }
    Ok(())
}
