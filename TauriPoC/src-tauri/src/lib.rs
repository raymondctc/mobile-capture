use capture_poc_core::{
    model::{Device, Limits, Snapshot},
    CaptureOwner,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::Manager;

type Owner = Arc<Mutex<CaptureOwner>>;
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
) -> Result<Snapshot, String> {
    with_owner(owner.inner().clone(), move |o| {
        o.start(serial, whole_device_consent, Limits::default())
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
    with_owner(owner.inner().clone(), |o| o.stop()).await
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
            let packaged_worker = executable
                .parent()
                .ok_or("Application executable has no directory")?
                .join(worker_name);
            let (adb, worker, root) = if fixture {
                (
                    PathBuf::from(std::env::var("MOBILE_CAPTURE_POC_FIXTURE_ADB")?),
                    PathBuf::from(std::env::var("MOBILE_CAPTURE_POC_WORKER")?),
                    PathBuf::from(std::env::var("MOBILE_CAPTURE_POC_FIXTURE_ROOT")?),
                )
            } else {
                (
                    PathBuf::from("adb"),
                    packaged_worker,
                    app.path().app_data_dir()?.join("sessions"),
                )
            };
            app.manage(Arc::new(Mutex::new(CaptureOwner::new(
                adb, worker, root, fixture,
            ))));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            discover, start, snapshot, marker, stop, export
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
