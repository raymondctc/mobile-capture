use crate::desktop::{Desktop, InstallationOutcome, State};
use capture_poc_core::model::Result;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tauri_plugin_updater::{Update, UpdaterExt};

const FEED: &str = "https://github.com/raymondctc/mobile-capture/releases/download/android-updates-stable/android-updater.json";
const PUBLIC_KEY: Option<&str> = option_env!("MOBILE_CAPTURE_UPDATER_PUBLIC_KEY");

#[derive(Clone, Serialize)]
pub struct Release {
    version: String,
    notes: String,
}
impl From<&Update> for Release {
    fn from(update: &Update) -> Self {
        Self {
            version: update.version.clone(),
            notes: update.body.clone().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum View {
    Unconfigured,
    Idle,
    Checking,
    Current,
    Available {
        release: Release,
        error: Option<String>,
    },
    Downloading {
        release: Release,
        received: u64,
        total: Option<u64>,
    },
    Ready {
        release: Release,
        error: Option<String>,
    },
    Installing {
        release: Release,
    },
    Error {
        message: String,
    },
    RepairRequired {
        release: Release,
        message: String,
    },
}

pub(super) struct Staged {
    update: Arc<Update>,
    bytes: Vec<u8>,
}
pub enum Updates {
    Unconfigured,
    Idle,
    Checking,
    Current,
    Available {
        update: Arc<Update>,
        error: Option<String>,
    },
    Downloading {
        release: Release,
        received: u64,
        total: Option<u64>,
    },
    Ready {
        staged: Arc<Staged>,
        error: Option<String>,
    },
    Installing {
        release: Release,
    },
    Error(String),
    RepairRequired {
        staged: Arc<Staged>,
        message: String,
    },
}
impl Updates {
    pub fn new() -> Self {
        if PUBLIC_KEY.is_some_and(|key| !key.trim().is_empty()) {
            Self::Idle
        } else {
            Self::Unconfigured
        }
    }
    fn view(&self) -> View {
        match self {
            Self::RepairRequired { staged, message } => View::RepairRequired {
                release: staged.update.as_ref().into(),
                message: message.clone(),
            },
            Self::Unconfigured => View::Unconfigured,
            Self::Idle => View::Idle,
            Self::Checking => View::Checking,
            Self::Current => View::Current,
            Self::Error(message) => View::Error {
                message: message.clone(),
            },
            Self::Available { update, error } => View::Available {
                release: update.as_ref().into(),
                error: error.clone(),
            },
            Self::Downloading {
                release,
                received,
                total,
            } => View::Downloading {
                release: release.clone(),
                received: *received,
                total: *total,
            },
            Self::Ready { staged, error } => View::Ready {
                release: staged.update.as_ref().into(),
                error: error.clone(),
            },
            Self::Installing { release } => View::Installing {
                release: release.clone(),
            },
        }
    }
}
fn read(state: &Desktop) -> Result<View> {
    Ok(state
        .updates
        .lock()
        .map_err(|_| "Update state lock failed")?
        .view())
}
#[tauri::command]
pub fn status(state: tauri::State<'_, State>) -> Result<View> {
    read(&state)
}

pub fn startup(app: tauri::AppHandle, state: State) {
    tauri::async_runtime::spawn(async move {
        let _ = check_inner(app, state).await;
    });
}
#[tauri::command]
pub async fn check(app: tauri::AppHandle, state: tauri::State<'_, State>) -> Result<View> {
    check_inner(app, state.inner().clone()).await
}
async fn check_inner(app: tauri::AppHandle, state: State) -> Result<View> {
    {
        let mut updates = state
            .updates
            .lock()
            .map_err(|_| "Update state lock failed")?;
        match &*updates {
            Updates::Idle | Updates::Current | Updates::Error(_) => *updates = Updates::Checking,
            _ => return Ok(updates.view()),
        }
    }
    let outcome = async {
        app.updater_builder()
            .pubkey(PUBLIC_KEY.ok_or("Updater is not configured")?.trim())
            .endpoints(vec![tauri::Url::parse(FEED).map_err(|e| e.to_string())?])
            .map_err(|e| e.to_string())?
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?
            .check()
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    let mut updates = state
        .updates
        .lock()
        .map_err(|_| "Update state lock failed")?;
    *updates = match outcome {
        Ok(Some(update)) => Updates::Available {
            update: Arc::new(update),
            error: None,
        },
        Ok(None) => Updates::Current,
        Err(message) => Updates::Error(message),
    };
    Ok(updates.view())
}
#[tauri::command]
pub async fn download(state: tauri::State<'_, State>, version: String) -> Result<View> {
    let state = state.inner().clone();
    let update = {
        let mut updates = state
            .updates
            .lock()
            .map_err(|_| "Update state lock failed")?;
        let Updates::Available { update, .. } = &*updates else {
            return Err("No update is available to download".into());
        };
        if update.version != version {
            return Err("The available update changed. Check its version first.".into());
        }
        let update = update.clone();
        *updates = Updates::Downloading {
            release: update.as_ref().into(),
            received: 0,
            total: None,
        };
        update
    };
    let progress_state = state.clone();
    let outcome = update
        .download(
            move |chunk, length| {
                if let Ok(mut updates) = progress_state.updates.lock() {
                    if let Updates::Downloading {
                        received, total, ..
                    } = &mut *updates
                    {
                        *received = received.saturating_add(chunk as u64);
                        *total = length;
                    }
                }
            },
            || {},
        )
        .await;
    let mut updates = state
        .updates
        .lock()
        .map_err(|_| "Update state lock failed")?;
    *updates = match outcome {
        Ok(bytes) => Updates::Ready {
            staged: Arc::new(Staged { update, bytes }),
            error: None,
        },
        Err(error) => Updates::Available {
            update,
            error: Some(error.to_string()),
        },
    };
    Ok(updates.view())
}

#[derive(Debug)]
enum InstallFailure {
    Retryable(String),
    RepairRequired(String),
}
fn native_install_error(error: tauri_plugin_updater::Error) -> InstallFailure {
    match error {
        tauri_plugin_updater::Error::PreviousAppNotRestored(backup) => InstallFailure::RepairRequired(format!("The update failed and the original app could not be restored. Quit and restore the app from {} or reinstall it before opening it again.", backup.display())),
        error => InstallFailure::Retryable(error.to_string()),
    }
}
fn install_reserved(
    desktop: &Desktop,
    install: impl FnOnce() -> std::result::Result<(), InstallFailure>,
) -> std::result::Result<(), InstallFailure> {
    let reservation = desktop
        .reserve_install()
        .map_err(InstallFailure::Retryable)?;
    let outcome = install();
    reservation
        .finish(match &outcome {
            Ok(()) => InstallationOutcome::Installed,
            Err(InstallFailure::Retryable(_)) => InstallationOutcome::RetryableFailure,
            Err(InstallFailure::RepairRequired(_)) => InstallationOutcome::RepairRequired,
        })
        .map_err(InstallFailure::Retryable)?;
    outcome
}

#[tauri::command]
pub async fn install(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
    version: String,
) -> Result<View> {
    let state = state.inner().clone();
    let staged = {
        let mut updates = state
            .updates
            .lock()
            .map_err(|_| "Update state lock failed")?;
        let Updates::Ready { staged, .. } = &*updates else {
            return Err("Download and verify the update first".into());
        };
        if staged.update.version != version {
            return Err("The downloaded update changed. Check its version first.".into());
        }
        let staged = staged.clone();
        *updates = Updates::Installing {
            release: staged.update.as_ref().into(),
        };
        staged
    };
    let installing = staged.clone();
    let desktop = state.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        install_reserved(&desktop, || {
            installing
                .update
                .install(&installing.bytes)
                .map_err(native_install_error)
        })
    })
    .await
    .map_err(|e| InstallFailure::Retryable(e.to_string()))
    .and_then(|result| result);
    match outcome {
        Ok(()) => app.restart(),
        Err(error) => {
            let mut updates = state
                .updates
                .lock()
                .map_err(|_| "Update state lock failed")?;
            *updates = match error {
                InstallFailure::Retryable(error) => Updates::Ready {
                    staged,
                    error: Some(error),
                },
                InstallFailure::RepairRequired(message) => {
                    Updates::RepairRequired { staged, message }
                }
            };
            Ok(updates.view())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        desktop::{test_desktop, Admission},
        operations::{self, Kind},
    };

    #[test]
    fn failed_native_rollback_excludes_capture_but_authorizes_user_quit() {
        let root = tempfile::tempdir().unwrap();
        let desktop = test_desktop(root.path());
        let error = native_install_error(tauri_plugin_updater::Error::PreviousAppNotRestored(
            root.path().join("previous.app"),
        ));
        assert!(matches!(
            install_reserved(&desktop, || Err(error)),
            Err(InstallFailure::RepairRequired(_))
        ));
        assert_eq!(
            *desktop.admission.lock().unwrap(),
            Admission::RepairRequired
        );
        assert!(desktop.accept().is_err());
        assert!(desktop.admission.lock().unwrap().exit_authorized());
        assert!(!desktop.begin_quit().unwrap());
        assert!(matches!(
            desktop.operations.view().unwrap(),
            crate::operations::View::Idle
        ));
    }

    #[test]
    fn frontend_permissions_allow_version_and_dragging_without_updater_access() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let permissions = capability["permissions"].as_array().unwrap();
        assert_eq!(permissions.len(), 2);
        assert!(permissions
            .iter()
            .any(|permission| permission == "core:app:allow-version"));
        assert!(permissions
            .iter()
            .any(|permission| permission == "core:window:allow-start-dragging"));
    }

    #[test]
    fn disabled_updater_configuration_initializes_without_publisher_credentials() {
        let configuration: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater: tauri_plugin_updater::Config =
            serde_json::from_value(configuration["plugins"]["updater"].clone()).unwrap();
        assert!(updater.require_signed_version);
        assert!(updater.pubkey.is_empty());
    }

    #[test]
    fn installer_excludes_commands_and_quit_then_retries_same_bytes_after_failure() {
        let root = tempfile::tempdir().unwrap();
        let desktop = test_desktop(root.path());
        let bytes = b"verified staged fixture";
        let mut attempts = 0;
        for succeeds in [false, true] {
            let outcome = install_reserved(&desktop, || {
                attempts += 1;
                assert_eq!(bytes, b"verified staged fixture");
                assert!(desktop.accept().is_err());
                assert!(!desktop.begin_quit().unwrap());
                assert!(desktop.reconnect().is_err());
                assert!(desktop.discover().is_err());
                assert!(desktop.applications("fixture-stream").is_err());
                assert!(desktop
                    .save_preferences(desktop.preferences.lock().unwrap().clone())
                    .is_err());
                assert!(operations::begin_save(desktop.clone(), None).is_err());
                assert!(operations::begin_setup(
                    desktop.clone(),
                    "fixture-stream".into(),
                    Kind::Certificate
                )
                .is_err());
                assert!(operations::retry_setup(desktop.clone(), "old".into()).is_err());
                if succeeds {
                    Ok(())
                } else {
                    Err(InstallFailure::Retryable("fixture installer failed".into()))
                }
            });
            assert_eq!(outcome.is_ok(), succeeds);
            assert_eq!(
                *desktop.admission.lock().unwrap(),
                if succeeds {
                    Admission::ExitAuthorized
                } else {
                    Admission::Running
                }
            );
        }
        assert_eq!(attempts, 2);
        assert!(desktop.accept().is_err());
    }

    #[test]
    fn busy_or_failed_owner_never_invokes_installer_and_panic_releases_reservation() {
        let root = tempfile::tempdir().unwrap();
        let desktop = test_desktop(root.path());
        assert!(desktop.begin_quit().unwrap());
        assert!(install_reserved(&desktop, || panic!("quit must exclude install")).is_err());
        *desktop.admission.lock().unwrap() = Admission::Running;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = install_reserved(&desktop, || panic!("fixture panic"));
        }));
        assert!(outcome.is_err());
        assert_eq!(*desktop.admission.lock().unwrap(), Admission::Running);
        *desktop.owner.lock().unwrap() = Err("owner unavailable".into());
        assert!(
            install_reserved(&desktop, || panic!("missing owner must exclude install")).is_err()
        );
    }
}
