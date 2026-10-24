use crate::desktop::{safe, State as DesktopState};
use capture_poc_core::{
    model::*,
    save::{CancelOutcome, SaveHandle},
    setup::{self, CheckDetails, SetupCapture},
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Save,
    Certificate,
    ConnectionCheck,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationResult {
    Save {
        archive: PathBuf,
        directory: PathBuf,
    },
    Certificate {
        serial: String,
        certificate_fingerprint: String,
        certificate_path: PathBuf,
        device_path: String,
        routing_restored: bool,
    },
    ConnectionCheck {
        serial: String,
        directory: PathBuf,
        url: String,
        certificate_fingerprint: String,
        https_observed: bool,
        phone_origin_confirmed: bool,
        routing_restored: bool,
        verified: bool,
        checked_at: String,
        expires_at: String,
        message: String,
    },
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum View {
    Idle,
    Saving {
        id: String,
        stage: String,
        completed: u64,
        total: Option<u64>,
        cancel_allowed: bool,
    },
    PreparingCertificate {
        id: String,
        stage: String,
        message: String,
    },
    CheckingConnection {
        id: String,
        url: String,
        certificate_fingerprint: String,
        https_observed: bool,
        phone_origin_confirmed: bool,
        remaining_seconds: u64,
    },
    Restoring {
        id: String,
        kind: Kind,
        message: String,
    },
    Finished {
        id: String,
        result: OperationResult,
    },
    Failed {
        id: String,
        kind: Kind,
        message: String,
        recovery_required: bool,
        directory: Option<PathBuf>,
    },
    Cancelled {
        id: String,
        kind: Kind,
        message: String,
    },
}

impl View {
    fn busy(&self) -> bool {
        matches!(
            self,
            Self::Saving { .. }
                | Self::PreparingCertificate { .. }
                | Self::CheckingConnection { .. }
                | Self::Restoring { .. }
                | Self::Failed {
                    recovery_required: true,
                    ..
                }
        )
    }
    fn id(&self) -> Option<&str> {
        match self {
            Self::Idle => None,
            Self::Saving { id, .. }
            | Self::PreparingCertificate { id, .. }
            | Self::CheckingConnection { id, .. }
            | Self::Restoring { id, .. }
            | Self::Finished { id, .. }
            | Self::Failed { id, .. }
            | Self::Cancelled { id, .. } => Some(id),
        }
    }
}

struct Job {
    view: View,
    cancelled: Arc<AtomicBool>,
    origin: Arc<AtomicBool>,
    save: Option<SaveHandle>,
    setup: Option<Arc<Mutex<SetupCapture>>>,
    history: Vec<OperationResult>,
}

pub struct Operations {
    job: Mutex<Job>,
    sequence: AtomicU64,
}

impl Operations {
    pub fn new() -> Self {
        Self {
            job: Mutex::new(Job {
                view: View::Idle,
                cancelled: Arc::new(AtomicBool::new(false)),
                origin: Arc::new(AtomicBool::new(false)),
                save: None,
                setup: None,
                history: vec![],
            }),
            sequence: AtomicU64::new(0),
        }
    }
    pub fn busy(&self) -> Result<bool> {
        Ok(self
            .job
            .lock()
            .map_err(|_| "Operation state lock failed")?
            .view
            .busy())
    }
    fn reserve(&self, kind: Kind) -> Result<(String, Arc<AtomicBool>, Arc<AtomicBool>)> {
        let mut job = self.job.lock().map_err(|_| "Operation state lock failed")?;
        if job.view.busy() {
            return Err("Finish or recover the current operation first".into());
        }
        let id = format!(
            "{}-{}",
            std::process::id(),
            self.sequence.fetch_add(1, Ordering::Relaxed)
        );
        job.cancelled = Arc::new(AtomicBool::new(false));
        job.origin = Arc::new(AtomicBool::new(false));
        job.save = None;
        job.setup = None;
        job.view = match kind {
            Kind::Save => View::Saving {
                id: id.clone(),
                stage: "stopping_sources".into(),
                completed: 0,
                total: None,
                cancel_allowed: true,
            },
            _ => View::PreparingCertificate {
                id: id.clone(),
                stage: "starting".into(),
                message: "Preparing the owned setup capture".into(),
            },
        };
        Ok((id, job.cancelled.clone(), job.origin.clone()))
    }
    fn update(&self, id: &str, view: View) -> Result<()> {
        let mut job = self.job.lock().map_err(|_| "Operation state lock failed")?;
        if job.view.id() != Some(id) {
            return Err("Operation is no longer current".into());
        }
        if let View::Finished { result, .. } = &view {
            if !matches!(result, OperationResult::Save { .. }) {
                job.history.push(result.clone());
                if job.history.len() > 64 {
                    job.history.remove(0);
                }
            }
        }
        job.view = view;
        Ok(())
    }
    pub fn view(&self) -> Result<View> {
        let (view, save, cancelled) = {
            let job = self.job.lock().map_err(|_| "Operation state lock failed")?;
            (
                job.view.clone(),
                job.save.clone(),
                job.cancelled.load(Ordering::Acquire),
            )
        };
        if let (View::Saving { id, .. }, Some(save)) = (&view, save) {
            let progress = save.progress()?;
            return Ok(View::Saving {
                id: id.clone(),
                stage: if cancelled && progress.cancel_allowed {
                    "cancellation_requested".into()
                } else {
                    progress.stage
                },
                completed: progress.completed,
                total: progress.total,
                cancel_allowed: progress.cancel_allowed && !cancelled,
            });
        }
        Ok(view)
    }
    pub fn saved_archive(&self, id: &str) -> Result<PathBuf> {
        match self.view()? {
            View::Finished {
                id: current,
                result: OperationResult::Save { archive, .. },
            } if current == id => Ok(archive),
            _ => Err("This completed save is no longer current".into()),
        }
    }
    pub fn facts(&self) -> Result<Vec<OperationResult>> {
        Ok(self
            .job
            .lock()
            .map_err(|_| "Operation state lock failed")?
            .history
            .clone())
    }
    pub fn cancel(&self, id: &str) -> Result<View> {
        let (save, cancelled) = {
            let job = self.job.lock().map_err(|_| "Operation state lock failed")?;
            if job.view.id() != Some(id) {
                return Err("Operation is no longer current".into());
            }
            if matches!(job.view, View::Finished { .. } | View::Cancelled { .. }) {
                return Ok(job.view.clone());
            }
            if !matches!(
                job.view,
                View::Saving { .. }
                    | View::PreparingCertificate { .. }
                    | View::CheckingConnection { .. }
            ) {
                return Err(
                    "This operation cannot be cancelled during restoration or after completion"
                        .into(),
                );
            }
            (job.save.clone(), job.cancelled.clone())
        };
        if let Some(save) = save {
            match save.cancel()? {
                CancelOutcome::Accepted | CancelOutcome::AlreadyFinished => {
                    cancelled.store(true, Ordering::Release)
                }
                CancelOutcome::PublicationCommitted => return self.view(),
            }
        } else {
            cancelled.store(true, Ordering::Release);
        }
        self.view()
    }
    pub fn confirm_origin(&self, id: &str, confirmed: bool) -> Result<View> {
        let mut job = self.job.lock().map_err(|_| "Operation state lock failed")?;
        if job.view.id() != Some(id) {
            return Err("Operation is no longer current".into());
        }
        if !matches!(job.view, View::CheckingConnection { .. }) {
            return Err("Phone origin can be confirmed only during the current check".into());
        }
        job.origin.store(confirmed, Ordering::Release);
        if let View::CheckingConnection {
            phone_origin_confirmed,
            ..
        } = &mut job.view
        {
            *phone_origin_confirmed = confirmed;
        }
        Ok(job.view.clone())
    }
    pub fn open_phone(&self, id: &str) -> Result<()> {
        let setup = {
            let job = self.job.lock().map_err(|_| "Operation state lock failed")?;
            if job.view.id() != Some(id) || !matches!(job.view, View::CheckingConnection { .. }) {
                return Err("No current connection check to open".into());
            }
            job.setup.clone().ok_or("Setup owner is missing")?
        };
        setup
            .lock()
            .map_err(|_| "Setup lock failed")?
            .open_browser()?;
        Ok(())
    }
    pub fn recovered_save(&self) -> Result<()> {
        let mut job = self.job.lock().map_err(|_| "Operation state lock failed")?;
        if let View::Failed {
            kind: Kind::Save,
            recovery_required,
            message,
            ..
        } = &mut job.view
        {
            *recovery_required = false;
            *message =
                "Sources stopped and device routing restored. Retry save to create the archive."
                    .into();
        }
        Ok(())
    }
}

pub fn begin_save(desktop: DesktopState, directory: Option<PathBuf>) -> Result<View> {
    let _admission = desktop
        .admission
        .lock()
        .map_err(|_| "Operation admission failed")?;
    if desktop.operations.busy()? {
        return Err("Finish or recover the current operation first".into());
    }
    desktop.with_owner(|owner| {
        if let Some(directory) = &directory {
            let directory = std::fs::canonicalize(directory).map_err(|e| e.to_string())?;
            if owner.directory() != Some(directory.as_path()) {
                if !safe(&owner.status()?) {
                    return Err("Stop the current capture before saving another session".into());
                }
                owner.open_saved(directory)?;
            }
        }
        owner.directory().ok_or("No capture session to save")?;
        Ok(())
    })?;
    let (id, cancelled, _) = desktop.operations.reserve(Kind::Save)?;
    let initial = desktop.operations.view()?;
    let state = desktop.clone();
    std::thread::spawn(move || save_worker(state, id, cancelled));
    Ok(initial)
}

fn save_worker(desktop: DesktopState, id: String, cancelled: Arc<AtomicBool>) {
    let mut directory = None;
    let mut stopped = false;
    let result = desktop.with_owner(|owner| {
        directory = owner.directory().map(std::path::Path::to_path_buf);
        if matches!(owner.status()?, Snapshot::Recording(_))
            && !matches!(owner.stop()?, StopOutcome::Ready(_))
        {
            return Err("Source shutdown or device routing restoration is not verified".into());
        }
        if !safe(&owner.status()?) {
            return Err("Recover source shutdown and device routing before saving".into());
        }
        stopped = true;
        if cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        let handle = owner.prepare_save()?;
        {
            let mut job = desktop
                .operations
                .job
                .lock()
                .map_err(|_| "Operation state lock failed")?;
            if cancelled.load(Ordering::Acquire) {
                handle.cancel()?;
            }
            job.save = Some(handle.clone());
        }
        match owner.export_with_control(&handle) {
            Ok(archive) => Ok(Some(archive)),
            Err(_) if handle.progress().is_ok_and(|p| p.stage == "cancelled") => Ok(None),
            Err(error) => Err(error),
        }
    });
    let view = match result {
        Ok(Some(archive)) => View::Finished {
            id: id.clone(),
            result: OperationResult::Save {
                directory: directory
                    .unwrap_or_else(|| archive.parent().unwrap_or(&archive).to_path_buf()),
                archive,
            },
        },
        Ok(None) => View::Cancelled {
            id: id.clone(),
            kind: Kind::Save,
            message: "Save cancelled before publication.".into(),
        },
        Err(message) => View::Failed {
            id: id.clone(),
            kind: Kind::Save,
            message,
            recovery_required: !stopped,
            directory,
        },
    };
    let _ = desktop.operations.update(&id, view);
}

pub fn begin_setup(desktop: DesktopState, serial: String, kind: Kind) -> Result<View> {
    let _admission = desktop
        .admission
        .lock()
        .map_err(|_| "Operation admission failed")?;
    desktop.ensure_idle()?;
    desktop.with_owner(|owner| owner.release_completed_control())?;
    let paths = CapturePaths {
        output_root: desktop.data.join("setup-sessions"),
        runtime_root: desktop.runtime_root.clone(),
    };
    let setup = Arc::new(Mutex::new(SetupCapture::open(
        paths,
        desktop.tools()?.selection(),
        desktop.fixture,
        serial.clone(),
    )?));
    let (id, cancelled, origin) = desktop.operations.reserve(kind)?;
    desktop
        .operations
        .job
        .lock()
        .map_err(|_| "Operation state lock failed")?
        .setup = Some(setup.clone());
    let initial = desktop.operations.view()?;
    let state = desktop.clone();
    std::thread::spawn(move || setup_worker(state, setup, id, serial, kind, cancelled, origin));
    Ok(initial)
}

fn setup_worker(
    desktop: DesktopState,
    setup: Arc<Mutex<SetupCapture>>,
    id: String,
    serial: String,
    kind: Kind,
    cancelled: Arc<AtomicBool>,
    origin: Arc<AtomicBool>,
) {
    let mut details: Option<CheckDetails> = None;
    let mut observed = false;
    let mut device_path = None;
    let action = (|| -> Result<()> {
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let check = setup.lock().map_err(|_| "Setup lock failed")?.start()?;
        details = Some(check.clone());
        match kind {
            Kind::Certificate => {
                desktop.operations.update(
                    &id,
                    View::PreparingCertificate {
                        id: id.clone(),
                        stage: "copying_public_certificate".into(),
                        message: "Copying the public CA certificate to Android Downloads".into(),
                    },
                )?;
                if !cancelled.load(Ordering::Acquire) {
                    device_path = Some(
                        setup
                            .lock()
                            .map_err(|_| "Setup lock failed")?
                            .copy_public_certificate()?,
                    );
                }
            }
            Kind::ConnectionCheck => {
                let started = Instant::now();
                loop {
                    observed = setup
                        .lock()
                        .map_err(|_| "Setup lock failed")?
                        .observe_https()?;
                    let confirmed = origin.load(Ordering::Acquire);
                    let remaining_seconds = 45u64.saturating_sub(started.elapsed().as_secs());
                    desktop.operations.update(
                        &id,
                        View::CheckingConnection {
                            id: id.clone(),
                            url: check.url.clone(),
                            certificate_fingerprint: check.certificate.fingerprint.clone(),
                            https_observed: observed,
                            phone_origin_confirmed: confirmed,
                            remaining_seconds,
                        },
                    )?;
                    if cancelled.load(Ordering::Acquire)
                        || remaining_seconds == 0
                        || (observed && confirmed)
                    {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            Kind::Save => return Err("Invalid setup operation".into()),
        }
        Ok(())
    })();
    let _ = desktop.operations.update(
        &id,
        View::Restoring {
            id: id.clone(),
            kind,
            message: "Waiting for device routing restoration and source shutdown".into(),
        },
    );
    let restoration = setup
        .lock()
        .map_err(|_| "Setup lock failed".to_owned())
        .and_then(|mut setup| setup.stop());
    let directory = setup.lock().ok().and_then(|setup| setup.directory());
    let restored = matches!(restoration, Ok(true));
    if restored {
        if let Ok(mut job) = desktop.operations.job.lock() {
            job.setup = None;
        }
    }
    drop(setup);
    let result = match (action, restoration) {
        (_, Err(message)) => Err(format!("Setup restoration failed. {message}")),
        (_, Ok(false)) => {
            Err("Device routing or source shutdown is not verified. Retry recovery.".into())
        }
        (Err(message), Ok(true)) => Err(message),
        (Ok(()), Ok(true)) => Ok(()),
    };
    let view = match result {
        Err(message) => View::Failed {
            id: id.clone(),
            kind,
            message,
            recovery_required: !restored,
            directory,
        },
        Ok(()) => match (kind, details, device_path) {
            (Kind::Certificate, Some(check), Some(device_path)) => View::Finished {
                id: id.clone(),
                result: OperationResult::Certificate {
                    serial,
                    certificate_fingerprint: check.certificate.fingerprint,
                    certificate_path: check.certificate.path,
                    device_path,
                    routing_restored: true,
                },
            },
            (Kind::ConnectionCheck, Some(check), _) => {
                let current = setup::public_certificate(&check.certificate.path).ok();
                let same_ca =
                    current.is_some_and(|ca| ca.fingerprint == check.certificate.fingerprint);
                let confirmed = origin.load(Ordering::Acquire);
                let verified = observed && confirmed && same_ca;
                let checked = chrono::Utc::now();
                View::Finished { id: id.clone(), result: OperationResult::ConnectionCheck {
                    serial, directory: check.directory, url: check.url, certificate_fingerprint: check.certificate.fingerprint,
                    https_observed: observed, phone_origin_confirmed: confirmed, routing_restored: true, verified,
                    checked_at: checked.to_rfc3339(), expires_at: (checked + chrono::Duration::minutes(10)).to_rfc3339(),
                    message: if verified { "Fresh HTTPS response and phone origin confirmed. Routing restored." } else if !same_ca { "Capture CA changed. Run a new connection check." } else if !observed { "No complete fresh HTTPS response was observed. Routing restored." } else { "HTTPS response observed, but phone origin was not confirmed. Routing restored." }.into(),
                } }
            }
            _ => View::Cancelled {
                id: id.clone(),
                kind,
                message: "Setup cancelled and routing restored.".into(),
            },
        },
    };
    let _ = desktop.operations.update(&id, view);
}

pub fn retry_setup(desktop: DesktopState, id: String) -> Result<View> {
    let _admission = desktop
        .admission
        .lock()
        .map_err(|_| "Operation admission failed")?;
    let (setup, kind) = {
        let mut job = desktop
            .operations
            .job
            .lock()
            .map_err(|_| "Operation state lock failed")?;
        let kind = match &job.view {
            View::Failed {
                id: current,
                kind,
                recovery_required: true,
                ..
            } if current == &id && *kind != Kind::Save => *kind,
            _ => return Err("No failed setup restoration to retry".into()),
        };
        let setup = job.setup.clone().ok_or("Recovery owner is missing")?;
        job.view = View::Restoring {
            id: id.clone(),
            kind,
            message: "Retrying the owned routing journal and source shutdown".into(),
        };
        (setup, kind)
    };
    let initial = desktop.operations.view()?;
    let state = desktop.clone();
    std::thread::spawn(move || {
        let result = setup
            .lock()
            .map_err(|_| "Setup lock failed".to_owned())
            .and_then(|mut setup| setup.recover());
        let directory = setup.lock().ok().and_then(|setup| setup.directory());
        let restored = matches!(result, Ok(true));
        if restored {
            if let Ok(mut job) = state.operations.job.lock() {
                job.setup = None;
            }
        }
        drop(setup);
        let view = if restored {
            View::Cancelled {
                id: id.clone(),
                kind,
                message: "Routing restored. Run device setup again to obtain a fresh result."
                    .into(),
            }
        } else {
            View::Failed {
                id: id.clone(),
                kind,
                message: result
                    .err()
                    .unwrap_or_else(|| "Routing or source shutdown remains unverified".into()),
                recovery_required: true,
                directory,
            }
        };
        let _ = state.operations.update(&id, view);
    });
    Ok(initial)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_recovery_and_restoration_hold_admission() {
        let operations = Operations::new();
        let (id, cancelled, _) = operations.reserve(Kind::ConnectionCheck).unwrap();
        operations.cancel(&id).unwrap();
        assert!(cancelled.load(Ordering::Acquire));
        operations
            .update(
                &id,
                View::Failed {
                    id: id.clone(),
                    kind: Kind::ConnectionCheck,
                    message: "unplugged".into(),
                    recovery_required: true,
                    directory: None,
                },
            )
            .unwrap();
        assert!(operations.reserve(Kind::Save).is_err());
        assert!(operations.confirm_origin(&id, true).is_err());
        operations
            .update(
                &id,
                View::Cancelled {
                    id: id.clone(),
                    kind: Kind::ConnectionCheck,
                    message: "restored".into(),
                },
            )
            .unwrap();
        assert!(operations.reserve(Kind::Save).is_ok());
    }
    #[test]
    fn late_cancel_reports_the_published_save_without_claiming_cancellation() {
        let operations = Operations::new();
        let (id, _, _) = operations.reserve(Kind::Save).unwrap();
        let archive = PathBuf::from("published.zip");
        operations
            .update(
                &id,
                View::Finished {
                    id: id.clone(),
                    result: OperationResult::Save {
                        archive: archive.clone(),
                        directory: PathBuf::from("session"),
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            operations.cancel(&id).unwrap(),
            View::Finished { .. }
        ));
        assert_eq!(operations.saved_archive(&id).unwrap(), archive);
        assert!(operations.saved_archive("other-save").is_err());
    }
}
