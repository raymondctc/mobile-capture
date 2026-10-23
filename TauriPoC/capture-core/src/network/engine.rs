use crate::{
    lease,
    model::{Result, ToolSelection},
    process::{self, Job},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct EngineConfig {
    pub directory: PathBuf,
    pub nonce: String,
    pub setup_nonce: String,
    pub forwarding: bool,
    #[serde(default)]
    pub policy: crate::model::NetworkPolicy,
}
pub(super) struct EngineStop {
    pub forced: bool,
    pub verified: bool,
}
pub(super) struct Engine {
    child: Child,
    input: Option<ChildStdin>,
    _job: Job,
    config: EngineConfig,
}
impl Engine {
    pub fn launch(
        tools: &ToolSelection,
        directory: &Path,
        ca: &Path,
        port: u16,
        forwarding: bool,
        policy: &crate::model::NetworkPolicy,
    ) -> Result<Self> {
        let executable = tools
            .mitmdump
            .as_ref()
            .ok_or("Configure an installed mitmdump 12.2.3 executable before network capture")?;
        if !executable.is_absolute() || !tools.addon.is_absolute() || !tools.addon.is_file() {
            return Err("Engine and addon must be absolute existing paths".into());
        }
        let mut check = Command::new(executable);
        process::quiet(&mut check);
        check.arg("--version");
        let output = process::output_bounded(&mut check, Duration::from_secs(5), None, 8192, 8192)
            .map_err(|error| format!("Engine version probe failed: {error}"))?;
        let text = String::from_utf8(output.stdout).map_err(|_| "Engine version was not UTF-8")?;
        if !output.status.success() || !pinned_version(&text) {
            return Err("Installed engine must be mitmdump 12.2.3".into());
        }
        lease::private_directory(ca)?;
        let config = EngineConfig {
            directory: directory.into(),
            nonce: lease::random_token()?,
            setup_nonce: lease::random_token()?[..32].to_owned(),
            forwarding,
            policy: policy.clone(),
        };
        let path = directory.join("runtime/engine-config.json");
        lease::write(&path, &config)?;
        for name in [
            "engine-ready.json",
            "engine-receipt.json",
            "engine-stop.json",
        ] {
            let _ = fs::remove_file(directory.join("runtime").join(name));
        }
        let mut command = Command::new(executable);
        process::quiet(&mut command);
        let mut child = command
            .args([
                "--quiet",
                "--listen-host",
                "127.0.0.1",
                "--listen-port",
                &port.to_string(),
                "--mode",
                "regular",
                "--set",
                &format!("confdir={}", ca.display()),
                "--set",
                "connection_strategy=lazy",
                "--set",
                "upstream_cert=false",
                "-s",
            ])
            .arg(&tools.addon)
            .env("CAPTURE_ENGINE_CONFIG", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Cannot start network engine {e}"))?;
        let job = Job::contain(&child).inspect_err(|_| {
            let _ = child.kill();
            let _ = child.wait();
        })?;
        let input = child.stdin.take();
        let mut engine = Self {
            child,
            input,
            _job: job,
            config,
        };
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            let ready =
                lease::read::<serde_json::Value>(&directory.join("runtime/engine-ready.json"));
            if ready.is_ok_and(|r| r["nonce"].as_str() == Some(&engine.config.nonce))
                && ca.join("mitmproxy-ca-cert.pem").is_file()
            {
                return Ok(engine);
            }
            if engine
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_some()
                || Instant::now() >= deadline
            {
                let _ = engine.child.kill();
                let _ = engine.child.wait();
                return Err(
                    "Engine did not publish authenticated readiness and public certificate".into(),
                );
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
    pub fn exited(&mut self) -> Result<bool> {
        Ok(self.child.try_wait().map_err(|e| e.to_string())?.is_some())
    }
    pub fn stop(&mut self) -> Result<EngineStop> {
        lease::write(
            &self.config.directory.join("runtime/engine-stop.json"),
            &serde_json::json!({"nonce":self.config.nonce}),
        )?;
        self.input.take();
        let status = match process::wait_bounded(&mut self.child, Duration::from_secs(8)) {
            Ok(status) => status,
            Err(_) => {
                self.child.kill().map_err(|e| e.to_string())?;
                self.child.wait().map_err(|e| e.to_string())?;
                return Ok(EngineStop {
                    forced: true,
                    verified: false,
                });
            }
        };
        let receipt = lease::read::<serde_json::Value>(
            &self.config.directory.join("runtime/engine-receipt.json"),
        );
        Ok(EngineStop {
            forced: false,
            verified: status.success()
                && receipt.is_ok_and(|r| {
                    r["nonce"].as_str() == Some(&self.config.nonce) && r["done"] == true
                }),
        })
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.input.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn pinned_version(output: &str) -> bool {
    output.lines().any(|line| {
        matches!(
            line.trim(),
            "Mitmproxy: 12.2.3" | "Mitmproxy: 12.2.3 binary"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::pinned_version;

    #[test]
    fn pinned_engine_accepts_official_package_and_binary_version_spellings() {
        for output in [
            "Mitmproxy: 12.2.3\nPython: 3.13",
            "Mitmproxy: 12.2.3 binary\n",
        ] {
            assert!(pinned_version(output));
        }
        for output in [
            "Mitmproxy: 12.2.4",
            "Mitmproxy: 12.2.30",
            "Mitmproxy: 12.2.3.dev",
            "Mitmproxy: 12.2.3 arbitrary",
        ] {
            assert!(!pinned_version(output));
        }
    }
}
