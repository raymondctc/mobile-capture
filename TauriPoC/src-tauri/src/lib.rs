use capture_poc_core::{
    model::{
        CapturePaths, CaptureSelection, Device, Limits, NetworkConsent, Snapshot, StartRequest,
        ToolSelection,
    },
    CaptureOwner,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::Manager;

type Owner = Arc<Mutex<CaptureOwner>>;
fn installed_mitmdump() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "mitmdump.exe"
    } else {
        "mitmdump"
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|directory| directory.join(name).canonicalize().ok())
}
async fn with_owner<T: Send + 'static>(
    owner: Owner,
    operation: impl FnOnce(&mut CaptureOwner) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut capture = owner.lock().map_err(|_| "Capture owner lock failed")?;
        operation(&mut capture)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn discover(owner: tauri::State<'_, Owner>) -> Result<Vec<Device>, String> {
    with_owner(owner.inner().clone(), |o| o.discover()).await
}
#[tauri::command]
async fn start(
    owner: tauri::State<'_, Owner>,
    serial: String,
    whole_device_consent: bool,
    network_consent: bool,
) -> Result<Snapshot, String> {
    with_owner(owner.inner().clone(), move |o| {
        o.start(StartRequest {
            serial,
            whole_device_consent,
            selection: if network_consent {
                CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true })
            } else {
                CaptureSelection::Logs
            },
            limits: Limits::default(),
        })
    })
    .await
}
#[tauri::command]
async fn snapshot(owner: tauri::State<'_, Owner>) -> Result<Snapshot, String> {
    with_owner(owner.inner().clone(), |o| o.snapshot()).await
}
#[tauri::command]
async fn marker(owner: tauri::State<'_, Owner>, text: String) -> Result<Snapshot, String> {
    with_owner(owner.inner().clone(), move |o| o.marker(text)).await
}
#[tauri::command]
async fn stop(owner: tauri::State<'_, Owner>) -> Result<Snapshot, String> {
    with_owner(owner.inner().clone(), |o| {
        o.stop()?;
        o.snapshot()
    })
    .await
}
#[tauri::command]
async fn recover(owner: tauri::State<'_, Owner>) -> Result<Snapshot, String> {
    with_owner(owner.inner().clone(), |o| {
        o.recover()?;
        o.snapshot()
    })
    .await
}
#[tauri::command]
async fn export(owner: tauri::State<'_, Owner>) -> Result<PathBuf, String> {
    with_owner(owner.inner().clone(), |o| o.export()).await
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let fixture = std::env::var("MOBILE_CAPTURE_POC_FIXTURE").ok().as_deref() == Some("1");
            let executable = std::env::current_exe()?;
            let worker_name = if cfg!(windows) {
                "collector-worker.exe"
            } else {
                "collector-worker"
            };
            let guardian_name = if cfg!(windows) {
                "capture-guardian.exe"
            } else {
                "capture-guardian"
            };
            let packaged_worker = executable
                .parent()
                .ok_or("Application executable has no directory")?
                .join(worker_name);
            let packaged_guardian = executable
                .parent()
                .ok_or("Application executable has no directory")?
                .join(guardian_name);
            let (adb, worker, guardian, output_root, runtime_root) = if fixture {
                (
                    PathBuf::from(std::env::var("MOBILE_CAPTURE_POC_FIXTURE_ADB")?),
                    PathBuf::from(std::env::var("MOBILE_CAPTURE_POC_WORKER")?),
                    PathBuf::from(std::env::var("MOBILE_CAPTURE_POC_GUARDIAN")?),
                    PathBuf::from(std::env::var("MOBILE_CAPTURE_POC_FIXTURE_ROOT")?),
                    Some(PathBuf::from(std::env::var(
                        "MOBILE_CAPTURE_POC_FIXTURE_RUNTIME_ROOT",
                    )?)),
                )
            } else {
                (
                    PathBuf::from("adb"),
                    packaged_worker,
                    packaged_guardian,
                    app.path().app_data_dir()?.join("sessions"),
                    None,
                )
            };
            let addon = app
                .path()
                .resolve("network_addon.py", tauri::path::BaseDirectory::Resource)?;
            let mitmdump = std::env::var_os("MOBILE_CAPTURE_POC_MITMDUMP")
                .map(PathBuf::from)
                .or_else(installed_mitmdump);
            let owner = CaptureOwner::open(
                CapturePaths {
                    output_root,
                    runtime_root,
                },
                ToolSelection {
                    adb,
                    worker,
                    guardian,
                    mitmdump,
                    addon,
                },
                fixture,
            )
            .map_err(std::io::Error::other)?;
            app.manage(Arc::new(Mutex::new(owner)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            discover, start, snapshot, marker, stop, recover, export
        ])
        .build(tauri::generate_context!())
        .expect("Cannot build the Mobile Capture PoC")
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. }
                    | tauri::RunEvent::WindowEvent {
                        event: tauri::WindowEvent::CloseRequested { .. },
                        ..
                    }
            ) {
                if let Some(owner) = app.try_state::<Owner>() {
                    if let Ok(mut owner) = owner.lock() {
                        let _ = owner.stop();
                    }
                }
            }
        });
}
