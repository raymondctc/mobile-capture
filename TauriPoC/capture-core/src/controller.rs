use crate::{
    model::*,
    network::{self, Action, Bootstrap, Client, Reply},
    owner::LogSession,
};
use std::{fs, path::Path};

pub struct CaptureOwner {
    paths: CapturePaths,
    tools: ToolSelection,
    fixture: bool,
    logs: LogSession,
    network: Option<Client>,
}
impl CaptureOwner {
    pub fn prior_shutdown_verified(paths: &CapturePaths, fixture: bool) -> Result<bool> {
        let (lock, _) = network::namespace(paths, fixture)?;
        let Some(directory) = network::prior(&lock)? else {
            return Ok(true);
        };
        Ok(network::admit_saved(&directory, &lock).is_ok())
    }
    pub fn open(paths: CapturePaths, tools: ToolSelection, fixture: bool) -> Result<Self> {
        let logs = LogSession::new(
            tools.adb.clone(),
            tools.worker.clone(),
            paths.output_root.clone(),
            fixture,
        );
        let mut owner = Self {
            paths,
            tools,
            fixture,
            logs,
            network: None,
        };
        let (lock, _) = network::namespace(&owner.paths, fixture)?;
        if let Some(directory) = network::prior(&lock)? {
            if directory.join("runtime/bootstrap.json").exists() {
                owner.network = Some(
                    Client::attach(directory.clone())
                        .unwrap_or_else(|_| Client::detached(directory)),
                );
            } else {
                network::verify_prior_completion(&directory)?;
            }
        }
        Ok(owner)
    }
    pub fn release_completed_control(&mut self) -> Result<()> {
        if self.network.is_some() {
            if !matches!(self.status()?, Snapshot::Stopped(_) | Snapshot::Failed(_)) {
                return Err(
                    "Stop and restore the active network capture before releasing control".into(),
                );
            }
            if !self
                .network
                .as_ref()
                .ok_or("Network capture missing")?
                .has_authenticated_completion()
            {
                let directory = self
                    .network
                    .as_ref()
                    .ok_or("Network capture missing")?
                    .directory
                    .clone();
                let (lock, _) = network::namespace(&self.paths, self.fixture)?;
                network::admit_saved(&directory, &lock)?;
                if !matches!(self.recover()?, StopOutcome::Ready(_)) {
                    return Err("Stopped capture source proof could not be reacquired".into());
                }
            }
            let client = self.network.as_mut().ok_or("Network capture missing")?;
            if !client.has_authenticated_completion() {
                return Err("Stopped capture has no authenticated completion".into());
            }
            let directory = client.directory.clone();
            let (lock, _) = network::namespace(&self.paths, self.fixture)?;
            client.release(&lock)?;
            self.network = Some(Client::detached(directory));
        } else if !matches!(
            self.logs.status()?,
            Snapshot::Idle { .. } | Snapshot::Stopped(_) | Snapshot::Failed(_)
        ) {
            return Err("Stop and verify the active log capture before releasing control".into());
        }
        Ok(())
    }
    pub fn open_saved(&mut self, directory: std::path::PathBuf) -> Result<Snapshot> {
        let directory = fs::canonicalize(directory).map_err(|e| e.to_string())?;
        if self.directory() == Some(directory.as_path()) {
            return self.status();
        }
        self.release_completed_control()?;
        if directory.join("runtime/bootstrap.json").exists() {
            let (lock, _) = network::namespace(&self.paths, self.fixture)?;
            network::admit_saved(&directory, &lock)?;
            let snapshot = network::offline_snapshot(&directory)?;
            if !matches!(snapshot, Snapshot::Stopped(_) | Snapshot::Failed(_)) {
                return Err("Saved network source completion is not available".into());
            }
            self.network = Some(Client::detached(directory));
            Ok(snapshot)
        } else {
            let snapshot = self.logs.open_saved(directory)?;
            self.network = None;
            Ok(snapshot)
        }
    }
    pub fn status(&mut self) -> Result<Snapshot> {
        if let Some(client) = &mut self.network {
            if client.missing_state()? {
                return Err(missing_state_error(client));
            }
            match client.request(Action::Status) {
                Ok(Reply::Snapshot(s)) => Ok(s),
                Ok(Reply::Error(e)) => Err(e),
                _ => network::offline_snapshot(&client.directory),
            }
        } else {
            self.logs.status()
        }
    }
    pub fn start(&mut self, request: StartRequest) -> Result<Snapshot> {
        request.limits.validate()?;
        request.network_policy.validate()?;
        request.context.validate()?;
        if request.selection.includes_logs()
            && matches!(request.log_scope, LogScope::WholeDevice)
            && !request.whole_device_consent
        {
            return Err("Explicit consent to whole-device Android logs is required".into());
        }
        if let Some(client) = &self.network {
            if client.missing_state()? {
                if !client.has_authenticated_completion() {
                    return Err(missing_state_error(client));
                }
            } else {
                let snapshot = self.snapshot()?;
                if !matches!(snapshot, Snapshot::Stopped(_) | Snapshot::Failed(_)) {
                    return Err("Restore and stop the prior network session first".into());
                }
            }
            let (lock, _) = network::namespace(&self.paths, self.fixture)?;
            if let Some(client) = &mut self.network {
                client.release(&lock)?;
            }
        }
        let resolved = if request.selection.includes_logs() {
            Some(crate::android::resolve(
                &self.tools.adb,
                &request.serial,
                &request.log_scope,
            )?)
        } else {
            None
        };
        match &request.selection {
            CaptureSelection::Logs => {
                let snapshot = self
                    .logs
                    .start(request, resolved.ok_or("Missing log scope resolution")?)?;
                self.network = None;
                Ok(snapshot)
            }
            CaptureSelection::LogsAndNetwork(consent) | CaptureSelection::Network(consent) => {
                if !consent.whole_device {
                    return Err(
                        "Explicit consent to whole-device HTTP(S) capture is required".into(),
                    );
                }
                if !crate::android::devices(&self.tools.adb)?
                    .iter()
                    .any(|d| d.serial == request.serial && d.state == DeviceState::Ready)
                {
                    return Err("Select an authorized ready Android device".into());
                }
                if !matches!(
                    self.logs.snapshot()?,
                    Snapshot::Idle { .. } | Snapshot::Stopped(_) | Snapshot::Failed(_)
                ) {
                    return Err("Stop the existing log session first".into());
                }
                fs::create_dir_all(&self.paths.output_root).map_err(|e| e.to_string())?;
                let directory = tempfile::Builder::new()
                    .prefix("tauri-network-")
                    .tempdir_in(&self.paths.output_root)
                    .map_err(|e| e.to_string())?
                    .keep();
                let directory = fs::canonicalize(directory).map_err(|e| e.to_string())?;
                let (lock, ca) = network::namespace(&self.paths, self.fixture)?;
                let mut tools = self.tools.clone();
                tools.adb = crate::process::absolute_executable(&tools.adb)?;
                let config = Bootstrap {
                    paths: self.paths.clone(),
                    tools,
                    fixture: self.fixture,
                    request,
                    directory,
                    lock,
                    ca,
                    recovery: false,
                    fixture_drop_initial_reply: false,
                    resolved_logs: resolved,
                    descriptor_version: 1,
                };
                let directory = config.directory.clone();
                match Client::launch(config) {
                    Ok(client) => self.network = Some(client),
                    Err(error) => {
                        if network::published_pending(&directory) {
                            self.network = Some(Client::detached(directory));
                        }
                        return Err(error);
                    }
                }
                self.snapshot()
            }
        }
    }
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        if let Some(client) = &mut self.network {
            if client.missing_state()? {
                return Err(missing_state_error(client));
            }
            match client.request(Action::Snapshot) {
                Ok(Reply::Snapshot(s)) => Ok(s),
                Ok(Reply::Error(e)) => Err(e),
                _ => network::offline_snapshot(&client.directory),
            }
        } else {
            self.logs.snapshot()
        }
    }
    pub fn marker(&mut self, text: String) -> Result<Snapshot> {
        if let Some(client) = &mut self.network {
            match client.request(Action::Marker(text))? {
                Reply::Snapshot(s) => Ok(s),
                Reply::Error(e) => Err(e),
                _ => Err("Unexpected marker response".into()),
            }
        } else {
            self.logs.marker(text)
        }
    }
    pub fn stop(&mut self) -> Result<StopOutcome> {
        if let Some(client) = &mut self.network {
            match client.request(Action::Stop) {
                Ok(Reply::Stop(s)) => Ok(s),
                Ok(Reply::Error(e)) => Err(e),
                _ => outcome(self.snapshot()?),
            }
        } else {
            outcome(self.logs.stop()?)
        }
    }
    pub fn recover(&mut self) -> Result<StopOutcome> {
        let client = self.network.as_mut().ok_or("No pending network session")?;
        if client.has_lease() {
            if let Ok(Reply::Stop(s)) = client.request(Action::Recover) {
                return Ok(s);
            }
        }
        let directory = client.directory.clone();
        if let Ok(attached) = Client::attach(directory.clone()) {
            self.network = Some(attached);
            return self.stop();
        }
        let mut config: Bootstrap = crate::lease::read(&directory.join("runtime/bootstrap.json"))?;
        config.recovery = true;
        self.network = Some(Client::launch(config)?);
        self.stop()
    }
    pub fn prepare_save(&mut self) -> Result<crate::save::SaveHandle> {
        if self.network.is_none() {
            return self.logs.prepare_save();
        }
        if !matches!(self.status()?, Snapshot::Stopped(_) | Snapshot::Failed(_)) {
            return Err("Save requires restored routing and verified source shutdown".into());
        }
        if !self
            .network
            .as_ref()
            .is_some_and(Client::has_authenticated_completion)
        {
            let directory = self
                .network
                .as_ref()
                .ok_or("Network capture missing")?
                .directory
                .clone();
            let (lock, _) = network::namespace(&self.paths, self.fixture)?;
            network::admit_saved(&directory, &lock)?;
            if !matches!(self.recover()?, StopOutcome::Ready(_)) {
                return Err("Stopped source writers are not quiescent".into());
            }
        }
        let client = self.network.as_ref().ok_or("Network capture missing")?;
        if !client.has_authenticated_completion() {
            return Err("Save requires authenticated completion".into());
        }
        crate::save::SaveHandle::create(&client.directory)
    }
    pub fn export_with_control(
        &mut self,
        control: &crate::save::SaveHandle,
    ) -> Result<std::path::PathBuf> {
        self.export_impl(Some(control))
    }
    pub fn export(&mut self) -> Result<std::path::PathBuf> {
        self.export_impl(None)
    }
    fn export_impl(
        &mut self,
        control: Option<&crate::save::SaveHandle>,
    ) -> Result<std::path::PathBuf> {
        if self.network.is_none() {
            return match control {
                Some(control) => self.logs.export_with_control(control),
                None => self.logs.export(),
            };
        }
        let client = self.network.as_mut().ok_or("Network capture missing")?;
        if let Some(control) = control {
            control.matches(&client.directory)?;
        }
        if client.missing_state()? {
            return Err(missing_state_error(client));
        }
        let command = || match control {
            Some(control) => Action::ExportControlled(control.id().into()),
            None => Action::Export,
        };
        let directory = client.directory.clone();
        let mut reply = client.request(command());
        if reply.is_err() && network::sealed_on_disk(&directory) {
            if !matches!(self.recover()?, StopOutcome::Ready(_)) {
                return Err("Stopped source writers are not quiescent".into());
            }
            reply = self
                .network
                .as_mut()
                .ok_or("Recovered capture missing")?
                .request(command());
        }
        match reply? {
            Reply::Archive(path) => Ok(path),
            Reply::Error(error) => Err(error),
            _ => Err("Unexpected export response".into()),
        }
    }
    pub fn directory(&self) -> Option<&Path> {
        self.network
            .as_ref()
            .map(|c| c.directory.as_path())
            .or_else(|| self.logs.directory())
    }
}
fn missing_state_error(client: &Client) -> String {
    if client.has_authenticated_completion() {
        format!(
            "Completed capture files are missing at {}. Evidence cannot be exported. You can start a new capture.",
            client.directory.display()
        )
    } else {
        format!(
            "Capture state is missing at {}. Routing restoration cannot be verified. Restore the session files and retry recovery before starting another capture.",
            client.directory.display()
        )
    }
}
fn outcome(snapshot: Snapshot) -> Result<StopOutcome> {
    match snapshot {
        Snapshot::Stopped(view) | Snapshot::Failed(view) => Ok(StopOutcome::Ready(view)),
        Snapshot::RecoveryRequired(view) => Ok(StopOutcome::RecoveryRequired(view)),
        Snapshot::UnverifiedShutdown(view) => Ok(StopOutcome::Unverified(view)),
        _ => Err("No stopped capture".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tools() -> ToolSelection {
        ToolSelection {
            adb: std::env::current_exe().unwrap(),
            worker: "unused-worker".into(),
            guardian: "unused-guardian".into(),
            mitmdump: None,
            addon: "unused-addon".into(),
        }
    }
    #[test]
    fn fresh_owner_refuses_missing_or_unverified_prior_metadata() {
        let states = [
            None,
            Some(b"invalid".to_vec()),
            Some(
                serde_json::to_vec(&json!({"phase":"recording","routing_restored":true})).unwrap(),
            ),
            Some(serde_json::to_vec(&json!({"phase":"saved","routing_restored":false})).unwrap()),
            Some(serde_json::to_vec(&json!({"phase":"saved"})).unwrap()),
            Some(serde_json::to_vec(&json!({"phase":"unknown","routing_restored":true})).unwrap()),
        ];
        for (index, state) in states.into_iter().enumerate() {
            let root = tempfile::tempdir().unwrap();
            let runtime = root.path().join("runtime");
            let prior = root.path().join("previous-session");
            fs::create_dir(&runtime).unwrap();
            if let Some(state) = state {
                fs::create_dir(&prior).unwrap();
                fs::write(prior.join("session.json"), state).unwrap();
            }
            let lock = runtime.join("owner.lock");
            fs::write(&lock, prior.to_str().unwrap()).unwrap();
            let locator = fs::read(&lock).unwrap();
            let paths = CapturePaths {
                output_root: root.path().join("output"),
                runtime_root: Some(runtime),
            };
            let result = CaptureOwner::open(paths.clone(), tools(), true);
            let Err(error) = result else {
                panic!("Fresh owner accepted unverified prior state {index}");
            };
            assert!(error.contains(&prior.display().to_string()), "{error}");
            assert!(error.contains("Restore"), "{error}");
            assert!(error.contains("owning app"), "{error}");
            assert_eq!(fs::read(lock).unwrap(), locator);
            assert!(!paths.output_root.exists());
        }
    }
    #[test]
    fn fresh_owner_accepts_completed_swift_metadata_and_attaches_own_bootstrap() {
        for phase in ["saved", "partial", "failed"] {
            let root = tempfile::tempdir().unwrap();
            let runtime = root.path().join("runtime");
            let prior = root.path().join("swift-session");
            fs::create_dir(&runtime).unwrap();
            fs::create_dir(&prior).unwrap();
            fs::write(runtime.join("owner.lock"), prior.to_str().unwrap()).unwrap();
            fs::write(
                prior.join("session.json"),
                serde_json::to_vec(&json!({
                    "phase": phase, "routing_restored": true
                }))
                .unwrap(),
            )
            .unwrap();
            let paths = CapturePaths {
                output_root: root.path().join("output"),
                runtime_root: Some(runtime),
            };
            let mut owner = CaptureOwner::open(paths.clone(), tools(), true).unwrap();
            assert!(matches!(owner.status().unwrap(), Snapshot::Idle { .. }));
            fs::create_dir(prior.join("runtime")).unwrap();
            fs::write(prior.join("runtime/bootstrap.json"), b"{}").unwrap();
            let owner = CaptureOwner::open(paths, tools(), true).unwrap();
            assert_eq!(owner.directory(), Some(prior.as_path()));
            assert!(!owner.network.unwrap().has_authenticated_completion());
        }
    }
    #[test]
    fn absent_namespace_is_idle_but_unreadable_prior_state_is_not_verified() {
        let root = tempfile::tempdir().unwrap();
        let paths = CapturePaths {
            output_root: root.path().join("sessions"),
            runtime_root: Some(root.path().join("runtime")),
        };
        assert!(CaptureOwner::prior_shutdown_verified(&paths, true).unwrap());
        std::fs::create_dir_all(paths.runtime_root.as_ref().unwrap()).unwrap();
        std::fs::write(
            paths.runtime_root.as_ref().unwrap().join("owner.lock"),
            root.path().join("missing-session").to_str().unwrap(),
        )
        .unwrap();
        assert!(!CaptureOwner::prior_shutdown_verified(&paths, true).unwrap());
    }
}
