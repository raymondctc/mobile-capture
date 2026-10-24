mod control;
mod engine;
mod route;

use crate::{
    lease::{self, FileLease},
    model::*,
    owner::LogCollector,
    timeline,
};
pub(crate) use control::{namespace, prior, Action, Bootstrap, Client, Reply};
use control::{Descriptor, Frame};
use engine::Engine;
use route::Route;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
struct Header {
    id: String,
    device: Device,
    started_at: String,
    phase: String,
    routing_restored: bool,
    interrupted: bool,
    error: Option<String>,
    completion: Option<NetworkCompletion>,
    archive: Option<PathBuf>,
}
struct VerifiedRestoration;
struct QuiescentSources {
    _logs: FileLease,
    _network: FileLease,
}
pub(crate) struct SealedEvidence {
    _restoration: VerifiedRestoration,
    _sources: QuiescentSources,
    completion: NetworkCompletion,
}
impl SealedEvidence {
    pub(crate) fn completion(&self) -> &NetworkCompletion {
        &self.completion
    }
}
struct Guardian {
    config: Bootstrap,
    header: Header,
    route: Option<Route>,
    engine: Option<Engine>,
    logs: Option<LogCollector>,
    markers: File,
    _lease: FileLease,
    quiescent: Option<QuiescentSources>,
    sealed: Option<SealedEvidence>,
}
impl Guardian {
    fn open(mut config: Bootstrap) -> Result<Self> {
        config.request.limits.validate()?;
        config.request.network_policy.validate()?;
        config.request.context.validate()?;
        if !config.recovery {
            config.descriptor_version = 1;
            match &config.request.selection {
                CaptureSelection::Network(consent) | CaptureSelection::LogsAndNetwork(consent)
                    if consent.whole_device => {}
                _ => {
                    return Err(
                        "Guardian requires explicit whole-device network capture consent".into(),
                    )
                }
            }
            if config.request.selection.includes_logs()
                && matches!(config.request.log_scope, LogScope::WholeDevice)
                && !config.request.whole_device_consent
            {
                return Err("Whole-device Android log consent is required".into());
            }
        }
        if !config.recovery {
            config.tools.adb = crate::process::absolute_executable(&config.tools.adb)?;
        } else if !config.tools.adb.is_absolute() {
            return Err("Saved ADB executable is not absolute".into());
        }
        let mut lock = FileLease::acquire_owner(&config.lock)?;
        if let Some(prior) = lock.prior()? {
            if prior != config.directory || !config.recovery {
                verify_prior_completion(&prior)?;
            }
        }
        lease::private_directory(&config.directory.join("runtime"))?;
        lease::private_directory(&config.directory.join("timeline"))?;
        let header = if config.recovery {
            let mut state: Header = lease::read(&config.directory.join("session.json"))?;
            if state.completion.is_none() {
                state.interrupted = true;
            }
            state.error = None;
            state
        } else {
            Header {
                id: config
                    .directory
                    .file_name()
                    .ok_or("Missing session name")?
                    .to_string_lossy()
                    .into_owned(),
                device: Device {
                    serial: config.request.serial.clone(),
                    name: config.request.serial.clone(),
                    state: DeviceState::Ready,
                },
                started_at: now(),
                phase: "preparing".into(),
                routing_restored: false,
                interrupted: false,
                error: None,
                completion: None,
                archive: None,
            }
        };
        lease::write(&config.directory.join("session.json"), &header)?;
        lease::write(&config.directory.join("runtime/bootstrap.json"), &config)?;
        if !config.recovery {
            let descriptor = SessionDescriptor {
                version: 1,
                id: header.id.clone(),
                device: header.device.clone(),
                started_at: header.started_at.clone(),
                selection: config.request.selection.clone(),
                log_scope: config.request.log_scope.clone(),
                limits: config.request.limits.clone(),
                network_policy: config.request.network_policy.clone(),
                fixture: config.fixture,
                context: config.request.context.clone(),
                log_attribution: config.resolved_logs.as_ref().map(|r| r.attribution.clone()),
            };
            lease::write(&config.directory.join("capture.json"), &descriptor)?;
        }
        lock.publish(&config.directory)?;
        let markers = OpenOptions::new()
            .create(true)
            .append(true)
            .open(config.directory.join("timeline/markers.jsonl"))
            .map_err(|e| e.to_string())?;
        let route = if config.directory.join("runtime/routing.json").exists() {
            Some(lease::read(&config.directory.join("runtime/routing.json"))?)
        } else {
            None
        };
        Ok(Self {
            config,
            header,
            route,
            engine: None,
            logs: None,
            markers,
            _lease: lock,
            quiescent: None,
            sealed: None,
        })
    }
    fn record(&self, event: serde_json::Value) {
        if !self.config.fixture {
            return;
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.config.directory.join("runtime/guardian-events.jsonl"))
        {
            if let Ok(mut bytes) = serde_json::to_vec(&event) {
                bytes.push(b'\n');
                let _ = file.write_all(&bytes);
                let _ = file.sync_data();
            }
        }
    }
    fn save(&self) -> Result<()> {
        lease::write(&self.config.directory.join("session.json"), &self.header)
    }
    fn start(&mut self) -> Result<()> {
        if self.config.recovery {
            self.quiesce()?;
            if self.route.as_ref().is_some_and(|r| !r.restored) {
                self.launch_forwarding()?;
            }
            let _ = self.stop();
            return Ok(());
        }
        let port = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| e.to_string())?
            .local_addr()
            .map_err(|e| e.to_string())?
            .port();
        self.engine = Some(Engine::launch(
            &self.config.tools,
            &self.config.directory,
            &self.config.ca,
            port,
            false,
            &self.config.request.network_policy,
        )?);
        if self.config.request.selection.includes_logs() {
            self.logs = Some(LogCollector::launch(
                &self.config.tools.worker,
                &WorkerConfig {
                    adb: self.config.tools.adb.clone(),
                    serial: self.config.request.serial.clone(),
                    directory: self.config.directory.clone(),
                    limits: self.config.request.limits.clone(),
                    source_lease: true,
                    log_scope: self.config.request.log_scope.clone(),
                    resolved_logs: self.config.resolved_logs.clone(),
                },
            )?);
            let deadline = Instant::now() + Duration::from_secs(5);
            while !self
                .config
                .directory
                .join("runtime/log-ready.json")
                .exists()
            {
                if Instant::now() >= deadline {
                    return Err("Log source did not acknowledge its writer lease".into());
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        let route = Route::prepare(
            self.config.tools.adb.clone(),
            self.config.request.serial.clone(),
            port,
            self.config.fixture,
        )?;
        route.save(&self.config.directory)?;
        self.route = Some(route);
        self.route
            .as_mut()
            .ok_or("Missing route")?
            .claim(&self.config.directory)?;
        self.header.phase = "recording".into();
        self.save()
    }
    fn launch_forwarding(&mut self) -> Result<()> {
        let port = self
            .route
            .as_ref()
            .ok_or("Recovery route missing")?
            .endpoint
            .strip_prefix("tcp:")
            .ok_or("Invalid saved endpoint")?
            .parse()
            .map_err(|_| "Invalid saved port")?;
        self.engine = Some(
            Engine::launch(
                &self.config.tools,
                &self.config.directory,
                &self.config.ca,
                port,
                true,
                &self.config.request.network_policy,
            )
            .map_err(|e| {
                format!("Recovery listener unavailable. No process was terminated. {e}")
            })?,
        );
        Ok(())
    }
    fn quiesce(&mut self) -> Result<()> {
        if self.quiescent.is_some() || self.sealed.is_some() {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let logs = FileLease::acquire(&self.config.directory.join("runtime/logs.writer"));
            if let Ok(logs) = logs {
                if let Ok(network) =
                    FileLease::acquire(&self.config.directory.join("runtime/network.writer"))
                {
                    self.quiescent = Some(QuiescentSources {
                        _logs: logs,
                        _network: network,
                    });
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err("Original source writer has not quiesced. Export is disabled".into());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
    fn coverage(&self) -> CoverageView {
        let mut coverage: CoverageView =
            lease::read(&self.config.directory.join("runtime/coverage.json")).unwrap_or_default();
        let certificate = self.config.ca.join("mitmproxy-ca-cert.pem");
        if certificate.is_file() {
            coverage.public_certificate = Some(certificate);
        }
        if let Ok(config) = lease::read::<engine::EngineConfig>(
            &self.config.directory.join("runtime/engine-config.json"),
        ) {
            coverage.synthetic_check_url = Some(format!(
                "https://mitm.it/mobile-capture-check/{}/https",
                config.setup_nonce
            ));
        }
        coverage
    }
    fn view(&self) -> Result<SessionView> {
        self.view_impl(true)
    }
    fn view_impl(&self, preview: bool) -> Result<SessionView> {
        let bytes =
            fs::metadata(self.config.directory.join("logs/device.log")).map_or(0, |m| m.len());
        let logs = if preview {
            crate::files::read(&self.config.directory, "logs/device.log")?.unwrap_or_default()
        } else {
            Vec::new()
        };
        let markers = if preview {
            crate::files::read(&self.config.directory, "timeline/markers.jsonl")?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let rawlogs = if self.header.completion.is_none() {
            &logs[..logs.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1)]
        } else {
            &logs
        };
        let mut events = timeline::events(rawlogs, &markers, &self.header.started_at);
        if preview {
            events.extend(timeline::network_events(
                &self.config.directory,
                &self.header.started_at,
                self.header.completion.is_none(),
            )?);
        }
        let log_count = events.iter().filter(|e| e.source == "logs").count();
        let marker_count = events.iter().filter(|e| e.source == "marker").count();
        timeline::retain_recent(&mut events);
        for event in &mut events {
            timeline::preview(event);
        }
        let log_notice = if !self.config.request.selection.includes_logs() {
            "Device logs were not selected.".to_owned()
        } else {
            match &self.config.request.log_scope {
                LogScope::App { application_id } => {
                    format!("Device logs are limited to {application_id}.")
                }
                LogScope::WholeDevice => {
                    "Device logs cover all apps and system processes.".to_owned()
                }
            }
        };
        let mut warnings = vec![format!("{log_notice} Network capture covers whole-device HTTP(S). HTTPS requires manual CA trust; certificate pinning can prevent observation."), "Timestamp proximity does not prove causation.".into()];
        if self.config.fixture {
            warnings.push("FIXTURE MODE. Compiled fake ADB evidence, not a phone.".into());
        }
        if self.header.interrupted {
            warnings.push("Guardian interrupted. The proxy listener may have stopped before recovery. Original evidence is partial.".into());
        }
        let network = if let Some(error) = &self.header.error {
            NetworkView::RecoveryRequired(RecoveryProblem {
                message: error.clone(),
                guardian_interrupted: self.header.interrupted,
            })
        } else if let Some(completion) = &self.header.completion {
            if completion.interrupted {
                NetworkView::Interrupted(completion.clone())
            } else {
                NetworkView::Stopped(completion.clone())
            }
        } else if self.header.phase == "preparing" {
            NetworkView::Preparing
        } else {
            NetworkView::Recording(self.coverage())
        };
        let view = SessionView {
            id: self.header.id.clone(),
            device: self.header.device.clone(),
            directory: self.config.directory.clone(),
            started_at: self.header.started_at.clone(),
            limits: self.config.request.limits.clone(),
            fixture: self.config.fixture,
            bytes,
            logs: log_count,
            markers: marker_count,
            events,
            receipt: lease::read(&self.config.directory.join("receipt.json")).ok(),
            archive: self.header.archive.clone(),
            warnings,
            network,
            descriptor_version: self.config.descriptor_version,
            selection: self.config.request.selection.clone(),
            log_scope: self.config.request.log_scope.clone(),
            network_policy: self.config.request.network_policy.clone(),
            context: self.config.request.context.clone(),
            log_attribution: self
                .config
                .resolved_logs
                .as_ref()
                .map(|r| r.attribution.clone()),
        };
        lease::write(&self.config.directory.join("runtime/view.json"), &view)?;
        Ok(view)
    }
    fn snapshot(&self) -> Result<Snapshot> {
        self.snapshot_impl(true)
    }
    fn status(&self) -> Result<Snapshot> {
        self.snapshot_impl(false)
    }
    fn snapshot_impl(&self, preview: bool) -> Result<Snapshot> {
        let view = self.view_impl(preview)?;
        Ok(
            if self.header.error.is_some() && self.header.routing_restored {
                Snapshot::UnverifiedShutdown(view)
            } else if self.header.error.is_some() {
                Snapshot::RecoveryRequired(view)
            } else if self.header.completion.is_some() {
                Snapshot::Stopped(view)
            } else {
                Snapshot::Recording(view)
            },
        )
    }
    fn stop(&mut self) -> Result<StopOutcome> {
        if let Some(completion) = self.header.completion.clone() {
            if self.sealed.is_none() {
                self.quiesce()?;
                if !self.header.routing_restored || self.route.as_ref().is_some_and(|r| !r.restored)
                {
                    return Err("Saved completion has no verified restoration".into());
                }
                self.sealed = Some(SealedEvidence {
                    _restoration: VerifiedRestoration,
                    _sources: self.quiescent.take().ok_or("Source quiescence missing")?,
                    completion,
                });
            }
            return Ok(StopOutcome::Ready(self.view()?));
        }
        self.header.phase = "stopping".into();
        self.header.error = None;
        self.save()?;
        if self
            .engine
            .as_mut()
            .is_some_and(|engine| engine.exited().unwrap_or(true))
            && self.route.as_ref().is_some_and(|route| !route.restored)
        {
            self.header.interrupted = true;
            self.engine.take();
            if let Err(error) = self.launch_forwarding() {
                return self.recovery_problem(error);
            }
        }
        if self.engine.is_none() && self.route.as_ref().is_some_and(|r| !r.restored) {
            if let Err(e) = self.launch_forwarding() {
                return self.recovery_problem(e);
            }
        }
        if let Some(route) = &mut self.route {
            if let Err(e) = route.restore(&self.config.directory) {
                return self.recovery_problem(e);
            }
        }
        self.header.routing_restored = true;
        self.save()?;
        let mut reasons = vec![];
        let mut forced = false;
        if let Some(logs) = &mut self.logs {
            if let Err(e) = logs.close() {
                reasons.push(e);
            }
        }
        self.logs.take();
        if let Some(engine) = &mut self.engine {
            match engine.stop() {
                Ok(value) => {
                    forced = value.forced;
                    if !value.verified {
                        reasons.push("Network engine has no verified graceful completion".into());
                    }
                }
                Err(e) => {
                    forced = true;
                    reasons.push(e);
                }
            }
        }
        self.engine.take();
        self.markers.sync_all().map_err(|e| e.to_string())?;
        if let Err(e) = self.quiesce() {
            self.header.error = Some(e);
            self.save()?;
            return Ok(StopOutcome::Unverified(self.view()?));
        }
        let facts: serde_json::Value =
            lease::read(&self.config.directory.join("network/incomplete.json"))
                .unwrap_or(serde_json::json!({"missing_artifact":true}));
        let receipt: Option<Receipt> =
            lease::read(&self.config.directory.join("receipt.json")).ok();
        if self.config.request.selection.includes_logs() && receipt.is_none() {
            reasons.push("Log source has no completion receipt".into());
        }
        if let Some(receipt) = &receipt {
            if self.config.request.selection.includes_logs()
                && (receipt.reason != StopReason::LeaseClosed
                    || receipt.forced
                    || receipt.incomplete_tail
                    || !receipt.stderr.is_empty())
            {
                reasons.push(format!(
                    "Log source ended {:?}. {}",
                    receipt.reason, receipt.stderr
                ));
            }
        }
        if timeline::network_events(&self.config.directory, &self.header.started_at, false)?
            .iter()
            .any(|event| matches!(event.kind.as_str(), "error" | "tls_error" | "parse_error"))
        {
            reasons.push("Network evidence contains HTTP, TLS, or parse errors".into());
        }
        let partial = self.header.interrupted
            || forced
            || facts["missing_artifact"] == true
            || facts["count"].as_u64().unwrap_or(0) > 0
            || facts["omitted_flows"].as_u64().unwrap_or(0) > 0
            || facts["missing_bodies"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
            || facts["errors"].as_array().is_some_and(|a| !a.is_empty())
            || !reasons.is_empty();
        if self.header.interrupted {
            reasons.push("Guardian died. Original capture continuity was interrupted".into());
        }
        let completion = NetworkCompletion {
            coverage: self.coverage(),
            routing_restored: true,
            partial,
            forced,
            interrupted: self.header.interrupted,
            reasons,
        };
        self.header.completion = Some(completion.clone());
        self.sealed = Some(SealedEvidence {
            _restoration: VerifiedRestoration,
            _sources: self.quiescent.take().ok_or("Source quiescence missing")?,
            completion,
        });
        self.header.phase = "stopped".into();
        self.save()?;
        Ok(StopOutcome::Ready(self.view()?))
    }
    fn recovery_problem(&mut self, message: String) -> Result<StopOutcome> {
        self.header.error = Some(message);
        self.save()?;
        Ok(StopOutcome::RecoveryRequired(self.view()?))
    }
    fn marker(&mut self, text: String) -> Result<Snapshot> {
        if self.header.phase != "recording" {
            return Err("Markers require an active capture".into());
        }
        if text.trim().is_empty() || text.len() > 4096 {
            return Err("Marker text must contain 1 to 4096 UTF-8 bytes".into());
        }
        if self.markers.metadata().map_err(|e| e.to_string())?.len() >= 1024 * 1024 {
            return Err("Marker byte limit reached".into());
        }
        let mut bytes = serde_json::to_vec(
            &serde_json::json!({"timestamp":now(),"text":text,"clock_uncertainty_ms":1}),
        )
        .map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        self.markers
            .write_all(&bytes)
            .and_then(|_| self.markers.sync_data())
            .map_err(|e| e.to_string())?;
        self.snapshot()
    }
    fn export(&mut self, control: Option<&crate::save::SaveHandle>) -> Result<PathBuf> {
        if let Some(control) = control {
            control.matches(&self.config.directory)?;
        }
        if self.config.descriptor_version == 1 {
            let public: serde_json::Value = serde_json::from_slice(
                &crate::files::read(&self.config.directory, "capture.json")?
                    .ok_or("Versioned public descriptor is missing")?,
            )
            .map_err(|e| e.to_string())?;
            let expected = SessionDescriptor {
                version: 1,
                id: self.header.id.clone(),
                device: self.header.device.clone(),
                started_at: self.header.started_at.clone(),
                selection: self.config.request.selection.clone(),
                log_scope: self.config.request.log_scope.clone(),
                limits: self.config.request.limits.clone(),
                network_policy: self.config.request.network_policy.clone(),
                fixture: self.config.fixture,
                context: self.config.request.context.clone(),
                log_attribution: self
                    .config
                    .resolved_logs
                    .as_ref()
                    .map(|r| r.attribution.clone()),
            };
            if public != serde_json::to_value(expected).map_err(|e| e.to_string())? {
                return Err(
                    "Public capture descriptor differs from the admitted source configuration"
                        .into(),
                );
            }
        }
        let sealed = self
            .sealed
            .as_ref()
            .ok_or("Restore routing and prove source quiescence before export")?;
        let receipt =
            lease::read(&self.config.directory.join("receipt.json")).unwrap_or_else(|_| {
                if !self.config.request.selection.includes_logs() {
                    Receipt {
                        reason: StopReason::LeaseClosed,
                        bytes: 0,
                        records: 0,
                        incomplete_tail: false,
                        child_reaped: true,
                        forced: false,
                        exit_code: None,
                        stderr: String::new(),
                        ended_at: now(),
                    }
                } else {
                    Receipt {
            reason: StopReason::ReadError,
            bytes: fs::metadata(self.config.directory.join("logs/device.log"))
                .map_or(0, |m| m.len()),
            records: 0,
            incomplete_tail: true,
            child_reaped: false,
            forced: true,
            exit_code: None,
            stderr: "Guardian interrupted; source lease proves writer quiescence without a receipt"
                .into(),
            ended_at: now(),
        }
                }
            });
        let archive = match control {
            Some(control) => crate::evidence::export_controlled(
                &self.config.directory,
                &self.header.id,
                &self.header.device,
                &self.header.started_at,
                &self.config.request.limits,
                &receipt,
                self.config.fixture,
                Some(sealed),
                control,
            )?,
            None => crate::evidence::export_network(
                &self.config.directory,
                &self.header.id,
                &self.header.device,
                &self.header.started_at,
                &self.config.request.limits,
                &receipt,
                self.config.fixture,
                sealed,
            )?,
        };
        self.header.archive = Some(archive.clone());
        self.save()?;
        Ok(archive)
    }
}

pub fn run(path: &Path) -> Result<()> {
    let result = run_inner(path);
    if let Err(error) = &result {
        let _ = lease::write(&path.with_extension("error"), error);
    }
    result
}
fn run_inner(path: &Path) -> Result<()> {
    let config: Bootstrap = lease::read(path)?;
    let directory = config.directory.clone();
    let mut guardian = match Guardian::open(config) {
        Ok(g) => g,
        Err(e) => {
            lease::write(&path.with_extension("error"), &e)?;
            return Err(e);
        }
    };
    guardian.record(
        serde_json::json!({"event":"guardian_started","recovery":guardian.config.recovery}),
    );
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let descriptor = Descriptor {
        port: listener.local_addr().map_err(|e| e.to_string())?.port(),
        token: lease::random_token()?,
    };
    lease::write(&directory.join("runtime/control.json"), &descriptor)?;
    lease::write(&path.with_extension("ready"), &true)?;
    let mut clients: Vec<TcpStream> = vec![];
    let mut attached = false;
    let mut owner_present = false;
    let startup = Instant::now();
    loop {
        if let Ok((mut stream, peer)) = listener.accept() {
            if !peer.ip().is_loopback() {
                continue;
            }
            stream.set_nonblocking(false).map_err(|e| e.to_string())?;
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|e| e.to_string())?;
            let frame = control::receive::<Frame>(&mut stream);
            if let Err(error) = &frame {
                guardian.record(serde_json::json!({"event":"frame_rejected","error":error}));
            }
            if let Ok(frame) = frame {
                if frame.token != descriptor.token {
                    guardian.record(serde_json::json!({"event":"authentication_refused"}));
                    continue;
                }
                let attach = matches!(frame.command, Action::Attach);
                let drop_reply = attach
                    && !attached
                    && guardian.config.fixture
                    && !guardian.config.recovery
                    && guardian.config.fixture_drop_initial_reply;
                let response = if attach && !attached {
                    attached = true;
                    match guardian.start() {
                        Ok(()) => guardian.snapshot().map(Reply::Snapshot),
                        Err(error) => {
                            let _ = guardian.recovery_problem(error.clone());
                            if guardian.route.is_none() {
                                let _ = guardian.stop();
                                Err(error)
                            } else {
                                guardian.snapshot().map(Reply::Snapshot)
                            }
                        }
                    }
                } else {
                    match frame.command {
                        Action::Attach | Action::Snapshot => {
                            guardian.snapshot().map(Reply::Snapshot)
                        }
                        Action::Status => guardian.status().map(Reply::Snapshot),
                        Action::Marker(text) => guardian.marker(text).map(Reply::Snapshot),
                        Action::Stop | Action::Recover => guardian.stop().map(Reply::Stop),
                        Action::Export => guardian.export(None).map(Reply::Archive),
                        Action::ExportControlled(id) => {
                            crate::save::SaveHandle::open(&guardian.config.directory, &id)
                                .and_then(|control| control.run(|| guardian.export(Some(&control))))
                                .map(Reply::Archive)
                        }
                    }
                };
                let sent = if drop_reply {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    guardian.record(serde_json::json!({"event":"fixture_dropped_initial_reply"}));
                    thread::sleep(Duration::from_secs(1));
                    Ok(())
                } else {
                    control::send(&mut stream, &response.unwrap_or_else(Reply::Error))
                };
                guardian.record(serde_json::json!({"event":"reply_sent","attach":attach,"phase":guardian.header.phase,"error":sent.err()}));
                if attach {
                    stream.set_nonblocking(true).map_err(|e| e.to_string())?;
                    clients.push(stream);
                    owner_present = true;
                }
            }
        }
        clients.retain_mut(|stream| {
            let mut byte=[0];
            let result=stream.read(&mut byte);
            if !matches!(&result,Err(e)if e.kind()==std::io::ErrorKind::WouldBlock){
                guardian.record(serde_json::json!({"event":"lease_read","result":match &result{Ok(n)=>format!("bytes:{n}"),Err(e)=>format!("{:?}: {e}",e.kind())},"phase":guardian.header.phase}));
            }
            matches!(result,Err(error)if matches!(error.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::Interrupted|std::io::ErrorKind::TimedOut))
        });
        if attached && owner_present && clients.is_empty() {
            owner_present = false;
            if guardian.sealed.is_none() {
                let _ = guardian.stop();
            }
        }
        if attached
            && (guardian.header.phase == "recording"
                || guardian.header.error.is_some() && !guardian.header.routing_restored)
        {
            let failed = guardian
                .engine
                .as_mut()
                .is_some_and(|e| e.exited().unwrap_or(true));
            if failed {
                let _ = guardian.stop();
            }
        }
        if attached && clients.is_empty() && guardian.header.completion.is_some() {
            guardian.record(serde_json::json!({"event":"exit_quiescent_no_clients"}));
            return Ok(());
        }
        if !attached && startup.elapsed() > Duration::from_secs(10) {
            guardian.record(serde_json::json!({"event":"exit_unattached_deadline"}));
            return Ok(());
        }
        thread::sleep(Duration::from_millis(15));
    }
}

pub(crate) fn offline_snapshot(directory: &Path) -> Result<Snapshot> {
    let mut view: SessionView = match lease::read(&directory.join("runtime/view.json")) {
        Ok(view) => view,
        Err(_) => {
            let header: Header = lease::read(&directory.join("session.json"))?;
            let config: Bootstrap = lease::read(&directory.join("runtime/bootstrap.json"))?;
            SessionView {
                id: header.id,
                device: header.device,
                directory: directory.to_owned(),
                started_at: header.started_at,
                limits: config.request.limits,
                fixture: config.fixture,
                bytes: fs::metadata(directory.join("logs/device.log")).map_or(0, |m| m.len()),
                logs: 0,
                markers: 0,
                events: vec![],
                receipt: None,
                archive: header.archive,
                warnings: vec![
                    "Source previews are unavailable until the guardian is recovered.".into(),
                ],
                descriptor_version: config.descriptor_version,
                selection: config.request.selection,
                log_scope: config.request.log_scope,
                network_policy: config.request.network_policy,
                context: config.request.context,
                log_attribution: config.resolved_logs.map(|r| r.attribution),
                network: NetworkView::RecoveryRequired(RecoveryProblem {
                    message: "Published capture needs guardian recovery".into(),
                    guardian_interrupted: true,
                }),
            }
        }
    };
    if !matches!(
        view.network,
        NetworkView::Stopped(_) | NetworkView::Interrupted(_)
    ) {
        view.network=NetworkView::RecoveryRequired(RecoveryProblem{message:"Guardian unreachable. Restore connection and stop; listener continuity may have been interrupted.".into(),guardian_interrupted:true});
        return Ok(Snapshot::RecoveryRequired(view));
    }
    Ok(Snapshot::Stopped(view))
}

pub(crate) fn published_pending(directory: &Path) -> bool {
    let bootstrap = lease::read::<Bootstrap>(&directory.join("runtime/bootstrap.json"));
    let header = lease::read::<Header>(&directory.join("session.json"));
    match (bootstrap, header) {
        (Ok(config), Ok(header)) if config.directory == directory => {
            !header.routing_restored || directory.join("runtime/routing.json").is_file()
        }
        _ => false,
    }
}

pub(crate) fn sealed_on_disk(directory: &Path) -> bool {
    lease::read::<Header>(&directory.join("session.json"))
        .is_ok_and(|header| header.routing_restored && header.completion.is_some())
}

pub(crate) fn verify_prior_completion(directory: &Path) -> Result<()> {
    #[derive(Deserialize)]
    struct PriorCompletion {
        phase: String,
        routing_restored: bool,
    }
    let verified = (|| {
        let state: PriorCompletion = lease::read(&directory.join("session.json"))?;
        if !state.routing_restored
            || !matches!(
                state.phase.as_str(),
                "stopped" | "saved" | "partial" | "failed"
            )
        {
            return Err("Prior session is not completed with restored routing".into());
        }
        Ok(())
    })();
    verified.map_err(|error: String| {
        format!(
            "Previous capture completion cannot be verified at {}. Restore the session files and retry recovery in the owning app before starting another capture. {error}",
            directory.display()
        )
    })
}

pub(crate) fn admit_saved(directory: &Path, lock: &Path) -> Result<()> {
    let config: Bootstrap = lease::read(&directory.join("runtime/bootstrap.json"))?;
    if config.directory != directory || config.lock != lock {
        return Err("Saved private capture identity does not match this control namespace".into());
    }
    let header: Header = lease::read(&directory.join("session.json"))?;
    if !header.routing_restored
        || !header
            .completion
            .as_ref()
            .is_some_and(|c| c.routing_restored)
    {
        return Err(
            "Saved network capture requires explicit recovery before it can be opened for export"
                .into(),
        );
    }
    if directory.join("runtime/routing.json").exists() {
        let route: Route = lease::read(&directory.join("runtime/routing.json"))?;
        if !route.restored {
            return Err("Private routing journal does not prove a restored capture".into());
        }
    }
    let _owner = FileLease::acquire_owner(lock)?;
    let _logs = FileLease::acquire(&directory.join("runtime/logs.writer"))?;
    let _network = FileLease::acquire(&directory.join("runtime/network.writer"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(root: &Path) -> Bootstrap {
        Bootstrap {
            paths: CapturePaths {
                output_root: root.join("output"),
                runtime_root: Some(root.join("private")),
            },
            tools: ToolSelection {
                adb: std::env::current_exe().unwrap(),
                worker: "unused-worker".into(),
                guardian: "unused-guardian".into(),
                mitmdump: None,
                addon: "unused-addon".into(),
            },
            fixture: true,
            request: StartRequest {
                serial: "unused-device".into(),
                whole_device_consent: true,
                selection: CaptureSelection::Network(NetworkConsent { whole_device: true }),
                limits: Limits::default(),
                log_scope: LogScope::default(),
                network_policy: NetworkPolicy::default(),
                context: CaptureContext::default(),
            },
            directory: root.join("new-session"),
            recovery: false,
            fixture_drop_initial_reply: false,
            ca: root.join("unused-ca"),
            lock: root.join("private/owner.lock"),
            resolved_logs: None,
            descriptor_version: 1,
        }
    }
    #[test]
    fn guardian_refuses_missing_or_unverified_prior_metadata_before_publish() {
        let states = [
            None,
            Some(json!({"phase":"saved"})),
            Some(json!({"phase":"saved","routing_restored":null})),
            Some(json!({"phase":"saved","routing_restored":false})),
            Some(json!({"phase":"recording","routing_restored":true})),
            Some(json!({"phase":"unknown","routing_restored":true})),
        ];
        for (index, state) in states.into_iter().enumerate() {
            let root = tempfile::tempdir().unwrap();
            let config = config(root.path());
            let prior = root.path().join("previous-session");
            if let Some(state) = state {
                lease::write(&prior.join("session.json"), &state).unwrap();
            }
            let mut owner = FileLease::acquire_owner(&config.lock).unwrap();
            owner.publish(&prior).unwrap();
            drop(owner);
            let locator = fs::read(&config.lock).unwrap();
            let lock = config.lock.clone();
            let directory = config.directory.clone();
            let result = Guardian::open(config);
            let Err(error) = result else {
                panic!("Guardian accepted unverified prior state {index}");
            };
            assert!(error.contains(&prior.display().to_string()), "{error}");
            assert!(error.contains("Restore"), "{error}");
            assert!(error.contains("owning app"), "{error}");
            assert_eq!(fs::read(&lock).unwrap(), locator);
            assert!(!directory.exists());
            assert!(FileLease::acquire_owner(&lock).is_ok());
        }
    }
    #[test]
    fn guardian_accepts_completed_prior_metadata_without_a_bootstrap() {
        for phase in ["saved", "partial", "failed", "stopped"] {
            let root = tempfile::tempdir().unwrap();
            let config = config(root.path());
            let prior = root.path().join("previous-session");
            lease::write(
                &prior.join("session.json"),
                &json!({
                    "phase":phase,"routing_restored":true
                }),
            )
            .unwrap();
            let mut owner = FileLease::acquire_owner(&config.lock).unwrap();
            owner.publish(&prior).unwrap();
            drop(owner);
            let mut guardian = Guardian::open(config).unwrap();
            assert_eq!(
                guardian._lease.prior().unwrap(),
                Some(guardian.config.directory.clone())
            );
            assert!(guardian.engine.is_none());
            assert!(guardian.route.is_none());
        }
    }
}
