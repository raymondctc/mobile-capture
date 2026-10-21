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
            }
        }
        Ok(owner)
    }
    pub fn discover(&self) -> Result<Vec<Device>> {
        crate::process::discover(&self.tools.adb)
    }
    pub fn start(&mut self, request: StartRequest) -> Result<Snapshot> {
        request.limits.validate()?;
        if !request.whole_device_consent {
            return Err("Explicit consent to whole-device Android logs is required".into());
        }
        if let Some(client) = &self.network {
            if missing_network_state(client)? {
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
        match &request.selection {
            CaptureSelection::Logs => {
                let snapshot = self.logs.start(
                    request.serial,
                    request.whole_device_consent,
                    request.limits,
                )?;
                self.network = None;
                Ok(snapshot)
            }
            CaptureSelection::LogsAndNetwork(consent) => {
                if !consent.whole_device {
                    return Err(
                        "Explicit consent to whole-device HTTP(S) capture is required".into(),
                    );
                }
                if !self
                    .discover()?
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
            if missing_network_state(client)? {
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
    pub fn export(&mut self) -> Result<std::path::PathBuf> {
        if self.network.is_none() {
            return self.logs.export();
        }
        let client = self.network.as_mut().ok_or("Network capture missing")?;
        if missing_network_state(client)? {
            return Err(missing_state_error(client));
        }
        let directory = client.directory.clone();
        let mut reply = client.request(Action::Export);
        if reply.is_err() && network::sealed_on_disk(&directory) {
            if !matches!(self.recover()?, StopOutcome::Ready(_)) {
                return Err("Stopped source writers are not quiescent".into());
            }
            reply = self
                .network
                .as_mut()
                .ok_or("Recovered capture missing")?
                .request(Action::Export);
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
fn missing_network_state(client: &Client) -> Result<bool> {
    for path in [
        client.directory.join("session.json"),
        client.directory.join("runtime/bootstrap.json"),
    ] {
        match fs::metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(error) => {
                return Err(format!(
                    "Cannot inspect capture state at {}. {error}",
                    path.display()
                ))
            }
        }
    }
    Ok(false)
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
