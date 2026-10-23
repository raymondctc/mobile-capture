use capture_poc_core::{
    android::AndroidApplication,
    doctor::{self as host_doctor, DoctorReport, DoctorRequest, DoctorSources},
    library::{
        BodyChunk, BodySide, DataChunk, EventDetail, SessionCatalog, SessionDescriptor,
        TimelinePage, TimelineQuery,
    },
    model::*,
    preferences::Preferences,
    setup::{self, SetupIdentity},
};
use std::{path::PathBuf, sync::atomic::Ordering};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::DialogExt;

mod desktop;
mod operations;
mod reports;
mod tools;
use desktop::{Desktop, Handoff, State as DesktopState};
use operations::{Kind, View};

async fn execute<T: Send + 'static>(
    state: DesktopState,
    action: impl FnOnce(DesktopState) -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || action(state))
        .await
        .map_err(|e| e.to_string())?
}
fn accepting(state: &DesktopState) -> Result<()> {
    if state.quitting.load(Ordering::Acquire) {
        Err("Quit is waiting for source shutdown, restoration, and saving".into())
    } else {
        Ok(())
    }
}
#[tauri::command]
async fn discover(state: tauri::State<'_, DesktopState>) -> Result<Vec<Device>> {
    execute(state.inner().clone(), |s| s.with_owner(|o| o.discover())).await
}
#[tauri::command]
async fn applications(
    state: tauri::State<'_, DesktopState>,
    serial: String,
) -> Result<Vec<AndroidApplication>> {
    execute(state.inner().clone(), move |s| {
        s.with_owner(|o| o.applications(&serial))
    })
    .await
}
#[tauri::command]
async fn start(state: tauri::State<'_, DesktopState>, request: StartRequest) -> Result<Snapshot> {
    execute(state.inner().clone(), move |s| {
        let _admission = s
            .admission
            .lock()
            .map_err(|_| "Operation admission failed")?;
        accepting(&s)?;
        if s.operations.busy()? {
            return Err("Finish or recover the current operation first".into());
        }
        let snapshot = s.with_owner(|o| o.start(request))?;
        if let Snapshot::Recording(session) = &snapshot {
            s.open_reader(session.directory.clone())?;
        }
        Ok(snapshot)
    })
    .await
}
#[tauri::command]
async fn snapshot(state: tauri::State<'_, DesktopState>) -> Result<Snapshot> {
    execute(state.inner().clone(), |s| s.with_owner(|o| o.status())).await
}
#[tauri::command]
async fn marker(state: tauri::State<'_, DesktopState>, text: String) -> Result<Snapshot> {
    execute(state.inner().clone(), move |s| {
        accepting(&s)?;
        if s.operations.busy()? {
            return Err("Finish the current operation first".into());
        }
        s.with_owner(|o| o.marker(text))
    })
    .await
}
#[tauri::command]
async fn stop(state: tauri::State<'_, DesktopState>) -> Result<Snapshot> {
    execute(state.inner().clone(), |s| {
        let _admission = s
            .admission
            .lock()
            .map_err(|_| "Operation admission failed")?;
        if s.operations.busy()? {
            return Err(
                "The current operation owns shutdown. Use its Finish or recovery action.".into(),
            );
        }
        s.with_owner(|o| {
            o.stop()?;
            o.status()
        })
    })
    .await
}
#[tauri::command]
async fn recover(state: tauri::State<'_, DesktopState>) -> Result<Snapshot> {
    execute(state.inner().clone(), |s| {
        let _admission = s
            .admission
            .lock()
            .map_err(|_| "Operation admission failed")?;
        let allowed = matches!(
            s.operations.view()?,
            View::Idle
                | View::Failed {
                    kind: Kind::Save,
                    ..
                }
                | View::Finished { .. }
                | View::Cancelled { .. }
        );
        if !allowed {
            return Err("Use the current setup operation's recovery action".into());
        }
        let snapshot = s.with_owner(|o| {
            let outcome = if matches!(o.status()?, Snapshot::UnverifiedShutdown(_)) {
                o.stop()?
            } else {
                o.recover()?
            };
            if !matches!(outcome, StopOutcome::Ready(_)) {
                return Err("Device routing or source shutdown remains unverified".into());
            }
            o.status()
        })?;
        if desktop::safe(&snapshot) {
            s.operations.recovered_save()?;
        }
        Ok(snapshot)
    })
    .await
}
#[tauri::command]
async fn sessions(state: tauri::State<'_, DesktopState>) -> Result<SessionCatalog> {
    execute(state.inner().clone(), |s| {
        capture_poc_core::library::EvidenceLibrary::catalog(&s.paths()?.output_root, 200)
    })
    .await
}
#[tauri::command]
async fn open_session(
    state: tauri::State<'_, DesktopState>,
    directory: PathBuf,
) -> Result<SessionDescriptor> {
    execute(state.inner().clone(), move |s| s.open_reader(directory)).await
}
#[tauri::command]
async fn timeline_query(
    state: tauri::State<'_, DesktopState>,
    query: TimelineQuery,
) -> Result<TimelinePage> {
    execute(state.inner().clone(), move |s| {
        let recording = s.owner.try_lock().ok().map(|mut owner| {
            owner
                .as_mut()
                .ok()
                .and_then(|o| o.status().ok())
                .and_then(|snapshot| {
                    if let Snapshot::Recording(session) = snapshot {
                        Some(session.directory)
                    } else {
                        None
                    }
                })
        });
        s.with_reader(|reader| {
            if let Some(recording) = recording {
                reader.set_live(
                    recording.as_deref() == Some(reader.descriptor().directory.as_path()),
                );
            }
            reader.refresh()?;
            reader.query(query)
        })
    })
    .await
}
#[tauri::command]
async fn event_detail(
    state: tauri::State<'_, DesktopState>,
    id: String,
) -> Result<Option<EventDetail>> {
    execute(state.inner().clone(), move |s| {
        s.with_reader(|reader| reader.event(&id))
    })
    .await
}
#[tauri::command]
async fn detail_chunk(
    state: tauri::State<'_, DesktopState>,
    id: String,
    offset: u64,
    limit: u32,
) -> Result<DataChunk> {
    execute(state.inner().clone(), move |s| {
        s.with_reader(|reader| reader.detail_chunk(&id, offset, limit))
    })
    .await
}
#[tauri::command]
async fn body_chunk(
    state: tauri::State<'_, DesktopState>,
    id: String,
    side: BodySide,
    offset: u64,
    limit: u32,
    decoded: bool,
) -> Result<BodyChunk> {
    execute(state.inner().clone(), move |s| {
        s.with_reader(|reader| {
            if decoded {
                reader.decoded_body_chunk(&id, side, offset, limit)
            } else {
                reader.body_chunk(&id, side, offset, limit)
            }
        })
    })
    .await
}
#[tauri::command]
async fn preferences(state: tauri::State<'_, DesktopState>) -> Result<Preferences> {
    Ok(state
        .preferences
        .lock()
        .map_err(|_| "Preferences lock failed")?
        .clone())
}
#[tauri::command]
async fn save_preferences(
    state: tauri::State<'_, DesktopState>,
    preferences: Preferences,
) -> Result<Preferences> {
    execute(state.inner().clone(), move |s| {
        accepting(&s)?;
        s.save_preferences(preferences)
    })
    .await
}
#[tauri::command]
async fn doctor(
    state: tauri::State<'_, DesktopState>,
    sources: DoctorSources,
) -> Result<DoctorReport> {
    execute(state.inner().clone(), move |s| {
        Ok(match s.tools() {
            Ok(tools) => host_doctor::run(&DoctorRequest {
                tools,
                storage_dir: s.paths()?.output_root,
                sources,
            }),
            Err(error) => DoctorReport::failure("payloadIntegrity", error),
        })
    })
    .await
}
#[tauri::command]
async fn setup_identity(state: tauri::State<'_, DesktopState>) -> Result<SetupIdentity> {
    execute(state.inner().clone(), |s| {
        setup::identity(&s.paths()?, s.fixture)
    })
    .await
}
#[tauri::command]
async fn pending_handoff(state: tauri::State<'_, DesktopState>) -> Result<Option<Handoff>> {
    Ok(state
        .pending_handoff
        .lock()
        .map_err(|_| "Launch handoff lock failed")?
        .take())
}
#[tauri::command]
async fn begin_save(
    state: tauri::State<'_, DesktopState>,
    directory: Option<PathBuf>,
) -> Result<View> {
    execute(state.inner().clone(), move |s| {
        accepting(&s)?;
        operations::begin_save(s, directory)
    })
    .await
}
#[tauri::command]
async fn begin_certificate(state: tauri::State<'_, DesktopState>, serial: String) -> Result<View> {
    execute(state.inner().clone(), move |s| {
        accepting(&s)?;
        operations::begin_setup(s, serial, Kind::Certificate)
    })
    .await
}
#[tauri::command]
async fn begin_connection_check(
    state: tauri::State<'_, DesktopState>,
    serial: String,
) -> Result<View> {
    execute(state.inner().clone(), move |s| {
        accepting(&s)?;
        operations::begin_setup(s, serial, Kind::ConnectionCheck)
    })
    .await
}
#[tauri::command]
async fn operation(state: tauri::State<'_, DesktopState>) -> Result<View> {
    state.operations.view()
}
#[tauri::command]
async fn cancel_operation(state: tauri::State<'_, DesktopState>, id: String) -> Result<View> {
    execute(state.inner().clone(), move |s| s.operations.cancel(&id)).await
}
#[tauri::command]
async fn confirm_check_origin(
    state: tauri::State<'_, DesktopState>,
    id: String,
    confirmed: bool,
) -> Result<View> {
    state.operations.confirm_origin(&id, confirmed)
}
#[tauri::command]
async fn open_check_on_phone(state: tauri::State<'_, DesktopState>, id: String) -> Result<()> {
    execute(state.inner().clone(), move |s| s.operations.open_phone(&id)).await
}
#[tauri::command]
async fn retry_setup_recovery(state: tauri::State<'_, DesktopState>, id: String) -> Result<View> {
    execute(state.inner().clone(), move |s| {
        operations::retry_setup(s, id)
    })
    .await
}
#[tauri::command]
async fn choose_folder(app: tauri::AppHandle) -> Result<Option<PathBuf>> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|p| p.into_path().map_err(|e| e.to_string()))
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn reveal(
    state: tauri::State<'_, DesktopState>,
    target: String,
    id: Option<String>,
) -> Result<()> {
    execute(state.inner().clone(), move |s| {
        let path = match target.as_str() {
            "session" => s.with_reader(|reader| Ok(reader.descriptor().directory.clone()))?,
            "raw" => s.with_reader(|reader| {
                Ok(reader
                    .raw_target(
                        id.as_deref()
                            .ok_or("Select an event to reveal raw evidence")?,
                    )?
                    .path)
            })?,
            "archive" if id.is_some() => s
                .operations
                .saved_archive(id.as_deref().ok_or("Completed save ID is missing")?)?,
            "archive" => s.with_reader(|reader| {
                Ok(reader.descriptor().directory.join("capture-package.zip"))
            })?,
            "certificate" => setup::identity(&s.paths()?, s.fixture)?
                .certificate_path
                .ok_or("No public capture certificate has been prepared")?,
            _ => return Err("Unsupported reveal target".into()),
        };
        if !path.try_exists().map_err(|e| e.to_string())? {
            return Err("The selected file is no longer present".into());
        }
        desktop::reveal_path(&path)
    })
    .await
}
#[tauri::command]
async fn copy_archive(
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> Result<Option<PathBuf>> {
    let state = state.inner().clone();
    execute(state, move |s| {
        let source =
            s.with_reader(|reader| Ok(reader.descriptor().directory.join("capture-package.zip")))?;
        let Some(destination) = app
            .dialog()
            .file()
            .set_file_name("capture-package.zip")
            .add_filter("Evidence ZIP", &["zip"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let destination = destination.into_path().map_err(|e| e.to_string())?;
        capture_poc_core::save::copy_evidence_archive(&source, &destination).map(Some)
    })
    .await
}
#[tauri::command]
async fn export_readiness_report(
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> Result<Option<PathBuf>> {
    execute(state.inner().clone(), move |s| {
        let Some(destination) = app
            .dialog()
            .file()
            .set_file_name("android-readiness.zip")
            .add_filter("Readiness ZIP", &["zip"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        reports::export(&s, &destination.into_path().map_err(|e| e.to_string())?).map(Some)
    })
    .await
}
#[tauri::command]
async fn open_updates(state: tauri::State<'_, DesktopState>) -> Result<()> {
    execute(state.inner().clone(), |s| {
        let _admission = s
            .admission
            .lock()
            .map_err(|_| "Operation admission failed")?;
        accepting(&s)?;
        s.ensure_idle()?;
        desktop::open_release_page()
    })
    .await
}
fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
fn quit(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<DesktopState>() else {
        return;
    };
    if state.quitting.swap(true, Ordering::AcqRel) {
        return;
    }
    let state = state.inner().clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            loop {
                match state.operations.view()? {
                    View::Failed {
                        recovery_required: true,
                        ..
                    } => return Err("Recover the failed operation before quitting".into()),
                    View::CheckingConnection { id, .. } | View::PreparingCertificate { id, .. } => {
                        state.operations.cancel(&id)?;
                    }
                    _ if !state.operations.busy()? => break,
                    _ => (),
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            let snapshot = state.with_owner(|o| o.status());
            match snapshot {
                Ok(Snapshot::Idle { .. }) => return Ok(()),
                Err(_)
                    if state
                        .owner
                        .lock()
                        .map_err(|_| "Owner lock failed")?
                        .is_err() =>
                {
                    if capture_poc_core::CaptureOwner::prior_shutdown_verified(
                        &state.paths()?,
                        state.fixture,
                    )? {
                        return Ok(());
                    }
                    return Err("Capture initialization failed and prior source shutdown is not verified. Repair tool preferences and recover the prior capture before quitting.".into());
                }
                Err(error) => return Err(error),
                Ok(Snapshot::RecoveryRequired(_) | Snapshot::UnverifiedShutdown(_)) => {
                    return Err(
                        "Restore device routing and verify source shutdown before quitting".into(),
                    )
                }
                Ok(Snapshot::Stopped(session) | Snapshot::Failed(session))
                    if session.archive.is_some() && desktop::safe_session(&session) =>
                {
                    return Ok(())
                }
                _ => (),
            }
            operations::begin_save(state.clone(), None)?;
            loop {
                match state.operations.view()? {
                    View::Finished { .. } => return Ok(()),
                    View::Failed { message, .. } => return Err(message),
                    View::Cancelled { .. } => {
                        return Err("Save was cancelled. Retry save before quitting.".into())
                    }
                    _ => std::thread::sleep(std::time::Duration::from_millis(250)),
                }
            }
        })();
        match result {
            Ok(()) => {
                state.exit_ready.store(true, Ordering::Release);
                app.exit(0);
            }
            Err(error) => {
                state.quitting.store(false, Ordering::Release);
                if let Ok(mut pending) = state.pending_handoff.lock() {
                    *pending = Some(Handoff {
                        device: None,
                        application_id: None,
                        session_id: None,
                        message: Some(format!("Quit could not finish. {error}")),
                    });
                }
                show(&app);
            }
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, arguments, _| {
            if let Some(state) = app.try_state::<DesktopState>() {
                for argument in arguments {
                    if argument.starts_with("mobilecapture-android:") {
                        state.queue_handoff(&argument);
                    }
                }
            }
            show(app);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = Desktop::open(app.handle()).map_err(std::io::Error::other)?;
            if let Some(urls) = app.deep_link().get_current()? {
                for url in urls {
                    state.queue_handoff(url.as_str());
                }
            }
            let handoff_state = state.clone();
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    handoff_state.queue_handoff(url.as_str());
                }
                show(&handle);
            });
            app.manage(state);
            let open = MenuItem::with_id(
                app,
                "open",
                "Open Mobile Capture Android",
                true,
                None::<&str>,
            )?;
            let quit_item =
                MenuItem::with_id(app, "quit", "Stop, Save and Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit_item])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("Mobile Capture Android")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show(app),
                    "quit" => quit(app),
                    _ => (),
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            discover,
            applications,
            start,
            snapshot,
            marker,
            stop,
            recover,
            sessions,
            open_session,
            timeline_query,
            event_detail,
            detail_chunk,
            body_chunk,
            preferences,
            save_preferences,
            doctor,
            setup_identity,
            pending_handoff,
            begin_save,
            begin_certificate,
            begin_connection_check,
            operation,
            cancel_operation,
            confirm_check_origin,
            open_check_on_phone,
            retry_setup_recovery,
            choose_folder,
            reveal,
            copy_archive,
            export_readiness_report,
            open_updates
        ])
        .build(tauri::generate_context!())
        .expect("Cannot build Mobile Capture Android")
        .run(|app, event| match event {
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } => {
                api.prevent_close();
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.hide();
                }
            }
            tauri::RunEvent::ExitRequested { api, .. }
                if app
                    .try_state::<DesktopState>()
                    .is_none_or(|s| !s.exit_ready.load(Ordering::Acquire)) =>
            {
                api.prevent_exit();
                quit(app);
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => show(app),
            _ => (),
        });
}
