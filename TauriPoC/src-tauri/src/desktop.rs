use capture_poc_core::{
    android::{self, AndroidApplication},
    library::EvidenceLibrary,
    model::*,
    preferences::{self, Preferences},
    toolchain::{self, ToolSource, Toolchain},
    CaptureOwner,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::Manager;

use crate::{operations::Operations, tools};

pub type State = Arc<Desktop>;
pub struct Desktop {
    pub admission: Mutex<()>,
    pub owner: Mutex<Result<CaptureOwner>>,
    pub reader: Mutex<Option<EvidenceLibrary>>,
    pub preferences: Mutex<Preferences>,
    pub operations: Operations,
    pub base_tools: Result<Toolchain>,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub preferences_path: PathBuf,
    pub fixture: bool,
    pub runtime_root: Option<PathBuf>,
    pub pending_handoff: Mutex<Option<Handoff>>,
    pub quitting: AtomicBool,
    pub exit_ready: AtomicBool,
}

#[derive(Clone, serde::Serialize)]
pub struct Handoff {
    pub device: Option<String>,
    pub application_id: Option<String>,
    pub session_id: Option<String>,
    pub message: Option<String>,
}

impl Desktop {
    pub fn open(app: &tauri::AppHandle) -> Result<State> {
        let fixture = std::env::var("MOBILE_CAPTURE_POC_FIXTURE").ok().as_deref() == Some("1");
        let environment = tools::DiscoveryEnv::from_process();
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let executable_directory = executable
            .parent()
            .ok_or("Application executable has no directory")?;
        let resource = app.path().resource_dir().map_err(|e| e.to_string())?;
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let worker = executable_directory.join(format!("collector-worker{suffix}"));
        let guardian = executable_directory.join(format!("capture-guardian{suffix}"));
        let addon = app
            .path()
            .resolve("network_addon.py", tauri::path::BaseDirectory::Resource)
            .map_err(|e| e.to_string())?;
        let data = if fixture {
            PathBuf::from(
                std::env::var("MOBILE_CAPTURE_POC_FIXTURE_ROOT")
                    .map_err(|_| "Fixture output root missing")?,
            )
            .parent()
            .ok_or("Fixture output root has no parent")?
            .join("desktop")
        } else {
            app.path().app_data_dir().map_err(|e| e.to_string())?
        };
        preferences::prepare_directory(&data)?;
        let output_root = if fixture {
            PathBuf::from(
                std::env::var("MOBILE_CAPTURE_POC_FIXTURE_ROOT")
                    .map_err(|_| "Fixture output root missing")?,
            )
        } else {
            data.join("sessions")
        };
        let runtime_root = if fixture {
            Some(PathBuf::from(
                std::env::var("MOBILE_CAPTURE_POC_FIXTURE_RUNTIME_ROOT")
                    .map_err(|_| "Fixture private namespace missing")?,
            ))
        } else {
            None
        };
        let preferences_path = data.join("android-preferences.json");
        let loaded = preferences::load(&preferences_path, output_root.clone());
        let preferences = loaded
            .clone()
            .unwrap_or_else(|_| Preferences::defaults(output_root));
        preferences::prepare_directory(&preferences.output_root)?;
        let base_tools = if fixture {
            Ok(Toolchain {
                adb: PathBuf::from(
                    std::env::var("MOBILE_CAPTURE_POC_FIXTURE_ADB")
                        .map_err(|_| "Fixture ADB missing")?,
                ),
                worker: std::env::var_os("MOBILE_CAPTURE_POC_WORKER")
                    .map(PathBuf::from)
                    .unwrap_or(worker),
                guardian: std::env::var_os("MOBILE_CAPTURE_POC_GUARDIAN")
                    .map(PathBuf::from)
                    .unwrap_or(guardian),
                mitmdump: std::env::var_os("MOBILE_CAPTURE_POC_MITMDUMP")
                    .map(PathBuf::from)
                    .or_else(|| tools::installed_mitmdump(&environment))
                    .unwrap_or_else(|| "mitmdump".into()),
                addon,
                source: ToolSource::Fixture,
                target: toolchain::host_target().into(),
            })
        } else if cfg!(debug_assertions)
            && std::env::var("MOBILE_CAPTURE_EXTERNAL_TOOLS")
                .ok()
                .as_deref()
                == Some("1")
        {
            Ok(Toolchain {
                adb: tools::installed_adb(&environment).ok_or("Development ADB is missing")?,
                mitmdump: tools::installed_mitmdump(&environment)
                    .unwrap_or_else(|| "mitmdump".into()),
                worker,
                guardian,
                addon: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../resources/network_addon.py"),
                source: ToolSource::External,
                target: toolchain::host_target().into(),
            })
        } else {
            tools::bundled_tools(&resource)
        };
        let paths = CapturePaths {
            output_root: preferences.output_root.clone(),
            runtime_root: runtime_root.clone(),
        };
        let owner = loaded
            .and_then(|_| selected_tools(&base_tools, &preferences))
            .and_then(|selected| CaptureOwner::open(paths, selected.selection(), fixture));
        let cache = data.join("timeline-cache");
        Ok(Arc::new(Self {
            admission: Mutex::new(()),
            owner: Mutex::new(owner),
            reader: Mutex::new(None),
            preferences: Mutex::new(preferences),
            operations: Operations::new(),
            base_tools,
            data,
            cache,
            preferences_path,
            fixture,
            runtime_root,
            pending_handoff: Mutex::new(None),
            quitting: AtomicBool::new(false),
            exit_ready: AtomicBool::new(false),
        }))
    }

    pub fn with_owner<T>(
        &self,
        operation: impl FnOnce(&mut CaptureOwner) -> Result<T>,
    ) -> Result<T> {
        let mut owner = self.owner.lock().map_err(|_| "Capture owner lock failed")?;
        operation(owner.as_mut().map_err(|error| error.clone())?)
    }

    pub fn discover(&self) -> Result<Vec<Device>> {
        let tools = self.tools()?;
        android::devices(&tools.adb)
    }

    pub fn applications(&self, serial: &str) -> Result<Vec<AndroidApplication>> {
        let tools = self.tools()?;
        android::applications(&tools.adb, serial)
    }

    pub fn reconnect(&self) -> Result<Snapshot> {
        let _admission = self
            .admission
            .lock()
            .map_err(|_| "Operation admission failed")?;
        if self.quitting.load(Ordering::Acquire) {
            return Err("Quit is waiting for safe shutdown".into());
        }
        if self.operations.busy()? {
            return Err("Finish or recover the current operation first".into());
        }
        let initialized = self.owner.lock().map_err(|_| "Owner lock failed")?.is_ok();
        if !initialized {
            let preferences = self
                .preferences
                .lock()
                .map_err(|_| "Preferences lock failed")?
                .clone();
            let selected = selected_tools(&self.base_tools, &preferences)?;
            let replacement = CaptureOwner::open(
                CapturePaths {
                    output_root: preferences.output_root,
                    runtime_root: self.runtime_root.clone(),
                },
                selected.selection(),
                self.fixture,
            )?;
            *self.owner.lock().map_err(|_| "Owner lock failed")? = Ok(replacement);
        }
        self.with_owner(|owner| owner.status())
    }

    pub fn paths(&self) -> Result<CapturePaths> {
        Ok(CapturePaths {
            output_root: self
                .preferences
                .lock()
                .map_err(|_| "Preferences lock failed")?
                .output_root
                .clone(),
            runtime_root: self.runtime_root.clone(),
        })
    }

    pub fn tools(&self) -> Result<Toolchain> {
        let preferences = self
            .preferences
            .lock()
            .map_err(|_| "Preferences lock failed")?;
        selected_tools(&self.base_tools, &preferences)
    }

    pub fn ensure_idle(&self) -> Result<()> {
        if self.operations.busy()? {
            return Err(
                "Finish the current save or device setup operation, including recovery, first"
                    .into(),
            );
        }
        self.with_owner(|owner| {
            if safe(&owner.status()?) {
                Ok(())
            } else {
                Err("Stop and restore the current capture first".into())
            }
        })
    }

    pub fn open_reader(
        &self,
        directory: PathBuf,
    ) -> Result<capture_poc_core::library::SessionDescriptor> {
        let live = match self.owner.try_lock() {
            Ok(mut owner) => owner.as_mut().ok().is_some_and(|owner| {
                owner.directory() == Some(directory.as_path())
                    && matches!(owner.status(), Ok(Snapshot::Recording(_)))
            }),
            Err(std::sync::TryLockError::WouldBlock) => true,
            Err(_) => return Err("Capture owner lock failed".into()),
        };
        let mut reader = self
            .reader
            .lock()
            .map_err(|_| "Evidence reader lock failed")?;
        if let Some(current) = reader.as_mut() {
            if current.descriptor().directory == directory {
                current.set_live(live);
                current.refresh()?;
                return Ok(current.descriptor().clone());
            }
        }
        let library = EvidenceLibrary::open(directory, self.cache.clone(), live)?;
        let descriptor = library.descriptor().clone();
        *reader = Some(library);
        Ok(descriptor)
    }

    pub fn with_reader<T>(
        &self,
        operation: impl FnOnce(&mut EvidenceLibrary) -> Result<T>,
    ) -> Result<T> {
        let mut reader = self
            .reader
            .lock()
            .map_err(|_| "Evidence reader lock failed")?;
        let reader = reader
            .as_mut()
            .ok_or("Open a capture session to inspect its evidence")?;
        operation(reader)
    }

    pub fn save_preferences(&self, preferences: Preferences) -> Result<Preferences> {
        let _admission = self
            .admission
            .lock()
            .map_err(|_| "Operation admission failed")?;
        if self.operations.busy()? {
            return Err("Finish or recover the current operation first".into());
        }
        let can_replace = self
            .with_owner(|owner| Ok(safe(&owner.status()?)))
            .unwrap_or(true);
        if !can_replace {
            return Err("Stop and restore capture before changing storage or tools".into());
        }
        preferences.validate()?;
        let selected = selected_tools(&self.base_tools, &preferences)?;
        preferences::prepare_directory(&preferences.output_root)?;
        self.with_owner(|owner| owner.release_completed_control())
            .or_else(|_| {
                if self.owner.lock().map_err(|_| "Owner lock failed")?.is_err() {
                    Ok::<(), String>(())
                } else {
                    Err("Cannot release prior capture ownership".into())
                }
            })?;
        let paths = CapturePaths {
            output_root: preferences.output_root.clone(),
            runtime_root: self.runtime_root.clone(),
        };
        let replacement = CaptureOwner::open(paths, selected.selection(), self.fixture)?;
        preferences::store(&self.preferences_path, &preferences)?;
        *self.owner.lock().map_err(|_| "Owner lock failed")? = Ok(replacement);
        *self
            .preferences
            .lock()
            .map_err(|_| "Preferences lock failed")? = preferences.clone();
        Ok(preferences)
    }

    pub fn queue_handoff(&self, raw: &str) {
        let handoff = match parse_handoff(raw) {
            Ok(handoff) => handoff,
            Err(error) => Handoff {
                device: None,
                application_id: None,
                session_id: None,
                message: Some(error),
            },
        };
        if let Ok(mut pending) = self.pending_handoff.lock() {
            *pending = Some(handoff);
        }
    }
}

fn selected_tools(base: &Result<Toolchain>, preferences: &Preferences) -> Result<Toolchain> {
    let mut tools = base.clone()?;
    for (path, selected) in [
        (&preferences.adb_override, &mut tools.adb),
        (&preferences.mitmdump_override, &mut tools.mitmdump),
    ] {
        if let Some(path) = path {
            *selected = preferences::executable_override(path)?;
            tools.source = ToolSource::External;
        }
    }
    Ok(tools)
}

pub fn safe(snapshot: &Snapshot) -> bool {
    match snapshot {
        Snapshot::Idle { .. } => true,
        Snapshot::Stopped(session) | Snapshot::Failed(session) => safe_session(session),
        _ => false,
    }
}
pub fn safe_session(session: &SessionView) -> bool {
    matches!(
        &session.network,
        NetworkView::NotSelected
            | NetworkView::Stopped(NetworkCompletion {
                routing_restored: true,
                ..
            })
            | NetworkView::Interrupted(NetworkCompletion {
                routing_restored: true,
                ..
            })
    )
}

pub fn parse_handoff(raw: &str) -> Result<Handoff> {
    if raw.len() > 4096 {
        return Err("Android launch link is too long".into());
    }
    let url = tauri::Url::parse(raw).map_err(|_| "Invalid Android launch link")?;
    if url.scheme() != "mobilecapture-android"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err("Unsupported Android launch link".into());
    }
    let mut values = std::collections::BTreeMap::new();
    for (key, value) in url.query_pairs() {
        if values.insert(key.to_string(), value.to_string()).is_some() {
            return Err("Duplicate Android launch parameter".into());
        }
    }
    if values.remove("v").as_deref() != Some("1") {
        return Err("Android launch links require version 1".into());
    }
    match url.host_str() {
        Some("open") => {
            let device = values
                .remove("device")
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 256
                        && !s.starts_with('-')
                        && !s.chars().any(char::is_whitespace)
                })
                .ok_or("Android launch link needs an explicit device")?;
            let application_id = values.remove("app");
            if application_id.as_ref().is_some_and(|s| {
                s.is_empty()
                    || s.len() > 255
                    || !s.bytes().enumerate().all(|(i, b)| {
                        b.is_ascii_alphanumeric()
                            || b == b'_'
                            || (i > 0 && matches!(b, b'.' | b'-'))
                    })
            }) {
                return Err("Android launch link has an invalid app identity".into());
            }
            if !values.is_empty() {
                return Err("Unknown Android launch parameter".into());
            }
            Ok(Handoff {
                device: Some(device),
                application_id,
                session_id: None,
                message: None,
            })
        }
        Some("session") => {
            let id = values
                .remove("id")
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 256
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                })
                .ok_or("Session launch link needs an opaque session ID")?;
            if !values.is_empty() {
                return Err("Unknown session launch parameter".into());
            }
            Ok(Handoff {
                device: None,
                application_id: None,
                session_id: Some(id),
                message: None,
            })
        }
        _ => Err("Unsupported Android launch action".into()),
    }
}

pub fn reveal_path(path: &Path) -> Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("/usr/bin/open");
        command.arg("-R").arg(path);
        command
    } else if cfg!(windows) {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(format!("/select,{}", path.display()));
        command
    } else {
        return Err("This desktop platform is not supported".into());
    };
    command.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn open_release_page() -> Result<()> {
    let url = "https://github.com/raymondctc/mobile-capture/releases";
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("/usr/bin/open").arg(url).spawn();
    #[cfg(windows)]
    let result = std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(not(any(target_os = "macos", windows)))]
    return Err("This desktop platform is not supported".into());
    #[cfg(any(target_os = "macos", windows))]
    result.map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn failed_owner_desktop(root: &Path) -> State {
        let fixture = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("fake-adb{}", std::env::consts::EXE_SUFFIX));
        assert!(
            fixture.is_file(),
            "Build the compiled ADB fixture before desktop tests with cargo build -p capture-poc-core --bin fake-adb --locked. Missing {}",
            fixture.display()
        );
        let tools_directory = root.join("tools");
        std::fs::create_dir(&tools_directory).unwrap();
        let adb = tools_directory.join(fixture.file_name().unwrap());
        std::fs::copy(fixture, &adb).unwrap();
        std::fs::write(
            tools_directory.join("fake-android-state.json"),
            br#"{"user":10,"uid":1010001,"shared":true}"#,
        )
        .unwrap();
        let runtime = root.join("runtime");
        std::fs::create_dir(&runtime).unwrap();
        std::fs::write(
            runtime.join("owner.lock"),
            root.join("prior-session").to_str().unwrap(),
        )
        .unwrap();
        let mut preferences = Preferences::defaults(root.join("sessions"));
        preferences.adb_override = Some(adb);
        let tools = Ok(Toolchain {
            adb: root.join("unselected-adb"),
            worker: root.join("unused-worker"),
            guardian: root.join("unused-guardian"),
            mitmdump: root.join("unused-mitmdump"),
            addon: root.join("unused-addon"),
            source: ToolSource::Fixture,
            target: toolchain::host_target().into(),
        });
        let owner = CaptureOwner::open(
            CapturePaths {
                output_root: preferences.output_root.clone(),
                runtime_root: Some(runtime.clone()),
            },
            selected_tools(&tools, &preferences).unwrap().selection(),
            true,
        );
        Arc::new(Desktop {
            admission: Mutex::new(()),
            owner: Mutex::new(owner),
            reader: Mutex::new(None),
            preferences: Mutex::new(preferences),
            operations: Operations::new(),
            base_tools: tools,
            data: root.join("desktop"),
            cache: root.join("cache"),
            preferences_path: root.join("preferences.json"),
            fixture: true,
            runtime_root: Some(runtime),
            pending_handoff: Mutex::new(None),
            quitting: AtomicBool::new(false),
            exit_ready: AtomicBool::new(false),
        })
    }

    #[test]
    fn android_queries_preserve_failed_capture_admission() {
        let root = tempfile::tempdir().unwrap();
        let desktop = failed_owner_desktop(root.path());
        let locator_path = root.path().join("runtime/owner.lock");
        let locator = std::fs::read(&locator_path).unwrap();
        let error = desktop
            .owner
            .lock()
            .unwrap()
            .as_ref()
            .err()
            .unwrap()
            .clone();
        assert!(
            error.contains("Previous capture completion cannot be verified"),
            "{error}"
        );
        assert!(error.contains("Private state is missing"), "{error}");

        for _ in 0..2 {
            let devices = desktop.discover().unwrap();
            for (serial, name, state) in [
                ("fixture-stream", "Fixture Android", DeviceState::Ready),
                ("fixture-offline", "fixture-offline", DeviceState::Offline),
                (
                    "fixture-denied",
                    "fixture-denied",
                    DeviceState::Unauthorized,
                ),
            ] {
                let device = devices
                    .iter()
                    .find(|device| device.serial == serial)
                    .unwrap();
                assert_eq!(device.name, name);
                assert_eq!(device.state, state);
            }
            let apps = desktop.applications("fixture-app-uid").unwrap();
            assert_eq!(apps.len(), 2);
            assert_eq!(apps[0].application_id, "com.fixture.app");
            assert_eq!(apps[1].application_id, "com.fixture.other");
            for app in apps {
                assert_eq!(app.user_id, 10);
                assert_eq!(app.uid, 1010001);
                assert!(app.shared_uid);
            }
            assert!(
                !CaptureOwner::prior_shutdown_verified(&desktop.paths().unwrap(), true).unwrap()
            );
            assert_eq!(desktop.ensure_idle().unwrap_err(), error);
            assert_eq!(desktop.reconnect().unwrap_err(), error);
            assert_eq!(
                desktop.with_owner::<()>(|_| panic!("Capture admission accepted a failed owner")),
                Err(error.clone())
            );
            for kind in [
                crate::operations::Kind::Certificate,
                crate::operations::Kind::ConnectionCheck,
            ] {
                let result =
                    crate::operations::begin_setup(desktop.clone(), "fixture-stream".into(), kind);
                assert_eq!(result.err().unwrap(), error);
            }
            assert_eq!(desktop.owner.lock().unwrap().as_ref().err(), Some(&error));
            assert_eq!(std::fs::read(&locator_path).unwrap(), locator);
            assert!(!root.path().join("prior-session").exists());
            assert!(!root.path().join("sessions").exists());
            assert!(!desktop.data.exists());
            assert!(!desktop.operations.busy().unwrap());
        }
        desktop.quitting.store(true, Ordering::Release);
        assert!(desktop.reconnect().unwrap_err().contains("Quit"));
    }

    #[test]
    fn android_queries_require_valid_selected_tools() {
        let root = tempfile::tempdir().unwrap();
        let mut desktop = failed_owner_desktop(root.path());
        for override_adb in [true, false] {
            let mut preferences = desktop.preferences.lock().unwrap();
            let path = Some(root.path().join("missing-executable"));
            if override_adb {
                preferences.adb_override = path;
            } else {
                preferences.adb_override = None;
                preferences.mitmdump_override = path;
            }
            drop(preferences);
            let error = desktop.tools().unwrap_err();
            assert_eq!(desktop.discover().unwrap_err(), error);
            assert_eq!(desktop.applications("fixture-app-uid").unwrap_err(), error);
        }
        Arc::get_mut(&mut desktop).unwrap().base_tools =
            Err("Bundled payload verification failed".into());
        assert_eq!(
            desktop.discover().unwrap_err(),
            "Bundled payload verification failed"
        );
        assert_eq!(
            desktop.applications("fixture-app-uid").unwrap_err(),
            "Bundled payload verification failed"
        );
    }

    #[test]
    fn reconnect_rechecks_restored_metadata_after_initialization_failed() {
        let root = tempfile::tempdir().unwrap();
        let runtime = root.path().join("runtime");
        let prior = root.path().join("prior-session");
        std::fs::create_dir(&runtime).unwrap();
        std::fs::write(runtime.join("owner.lock"), prior.to_str().unwrap()).unwrap();
        let desktop = Desktop {
            admission: Mutex::new(()),
            owner: Mutex::new(Err("Previous capture state is missing".into())),
            reader: Mutex::new(None),
            preferences: Mutex::new(Preferences::defaults(root.path().join("sessions"))),
            operations: Operations::new(),
            base_tools: Ok(Toolchain {
                adb: root.path().join("adb"),
                worker: root.path().join("worker"),
                guardian: root.path().join("guardian"),
                mitmdump: root.path().join("mitmdump"),
                addon: root.path().join("addon"),
                source: ToolSource::Fixture,
                target: toolchain::host_target().into(),
            }),
            data: root.path().join("desktop"),
            cache: root.path().join("cache"),
            preferences_path: root.path().join("preferences.json"),
            fixture: true,
            runtime_root: Some(runtime),
            pending_handoff: Mutex::new(None),
            quitting: AtomicBool::new(false),
            exit_ready: AtomicBool::new(false),
        };
        assert!(desktop.reconnect().is_err());
        assert!(desktop.owner.lock().unwrap().is_err());
        std::fs::create_dir(&prior).unwrap();
        std::fs::write(
            prior.join("session.json"),
            br#"{"phase":"saved","routing_restored":true}"#,
        )
        .unwrap();
        assert!(matches!(
            desktop.reconnect().unwrap(),
            Snapshot::Idle { .. }
        ));
        assert!(desktop.owner.lock().unwrap().is_ok());
        desktop.quitting.store(true, Ordering::Release);
        assert!(desktop.reconnect().unwrap_err().contains("Quit"));
    }

    #[test]
    fn failed_session_open_preserves_previous_inspected_evidence() {
        let temporary = tempfile::tempdir().unwrap();
        let data = temporary.path().join("desktop");
        std::fs::create_dir_all(&data).unwrap();
        let evidence = temporary.path().join("session");
        for child in ["logs", "network", "timeline"] {
            std::fs::create_dir_all(evidence.join(child)).unwrap();
        }
        std::fs::write(evidence.join("capture.json"), br#"{"version":1,"id":"reader-proof","started_at":"2026-10-23T00:00:00Z","selection":{"mode":"logs"},"context":{"title":"Fixture","notes":"","steps":[]}}"#).unwrap();
        std::fs::write(
            evidence.join("logs/device.log"),
            b"1700000000.000001 1 2 I Tag: preserved\n",
        )
        .unwrap();
        let desktop = Desktop {
            admission: Mutex::new(()),
            owner: Mutex::new(Err("Tools unavailable".into())),
            reader: Mutex::new(None),
            preferences: Mutex::new(Preferences::defaults(temporary.path().join("sessions"))),
            operations: Operations::new(),
            base_tools: Err("Tools unavailable".into()),
            cache: data.join("cache"),
            preferences_path: data.join("preferences.json"),
            data,
            fixture: true,
            runtime_root: Some(temporary.path().join("runtime")),
            pending_handoff: Mutex::new(None),
            quitting: AtomicBool::new(false),
            exit_ready: AtomicBool::new(false),
        };
        assert_eq!(
            desktop.open_reader(evidence.clone()).unwrap().id,
            "reader-proof"
        );
        assert!(desktop
            .open_reader(temporary.path().join("missing"))
            .is_err());
        assert_eq!(
            desktop
                .with_reader(|reader| Ok(reader.query(Default::default())?.rows.len()))
                .unwrap(),
            1
        );
        {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(evidence.join("logs/device.log"))
                .unwrap()
                .write_all(b"1700000001.000001 1 2 I Tag: unfinished")
                .unwrap();
            let _export_owner = desktop.owner.lock().unwrap();
            assert_eq!(
                desktop.open_reader(evidence.clone()).unwrap().id,
                "reader-proof"
            );
            assert_eq!(
                desktop
                    .with_reader(|reader| Ok(reader.query(Default::default())?.rows.len()))
                    .unwrap(),
                1
            );
        }
        assert_eq!(desktop.open_reader(evidence).unwrap().id, "reader-proof");
        assert_eq!(
            desktop
                .with_reader(|reader| Ok(reader.query(Default::default())?.rows.len()))
                .unwrap(),
            2
        );
    }

    #[test]
    fn launch_links_cannot_start_capture_or_select_paths() {
        assert_eq!(
            parse_handoff("mobilecapture-android://open?v=1&device=fixture&app=com.example.app")
                .unwrap()
                .device
                .as_deref(),
            Some("fixture")
        );
        assert!(parse_handoff("mobilecapture-android://open?v=1&device=fixture&start=1").is_err());
        assert!(parse_handoff("mobilecapture-android://session?v=1&id=..%2Fprivate").is_err());
        assert!(parse_handoff("mobilecapture://open?v=1&device=fixture").is_err());
        assert!(parse_handoff("mobilecapture-android://open?v=1&device=a&device=b").is_err());
    }
}
