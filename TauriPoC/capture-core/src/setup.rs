use crate::{files, model::*, network, CaptureOwner};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicCertificate {
    pub path: PathBuf,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetupIdentity {
    pub certificate_fingerprint: Option<String>,
    pub certificate_path: Option<PathBuf>,
    pub checked_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckDetails {
    pub directory: PathBuf,
    pub url: String,
    pub certificate: PublicCertificate,
}

pub fn identity(paths: &CapturePaths, fixture: bool) -> Result<SetupIdentity> {
    let (_, ca) = network::namespace(paths, fixture)?;
    let path = ca.join("mitmproxy-ca-cert.pem");
    let certificate = if path.try_exists().map_err(|e| e.to_string())? {
        Some(public_certificate(&path)?)
    } else {
        None
    };
    Ok(SetupIdentity {
        certificate_fingerprint: certificate.as_ref().map(|c| c.fingerprint.clone()),
        certificate_path: certificate.map(|c| c.path),
        checked_at: now(),
    })
}

fn certificate_bytes(path: &Path) -> Result<Vec<u8>> {
    let parent = path.parent().ok_or("Certificate has no directory")?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid public certificate name")?;
    let mut file = files::open(parent, name).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Public certificate is not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("Public certificate exceeds 64 KiB".into());
    }
    Ok(bytes)
}

fn certificate_der(pem: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(pem).map_err(|_| "Public certificate is not PEM text")?;
    let text = text.trim();
    let middle = text
        .strip_prefix("-----BEGIN CERTIFICATE-----")
        .and_then(|s| s.strip_suffix("-----END CERTIFICATE-----"))
        .ok_or("Only a public PEM certificate may be transferred")?;
    if middle.contains("-----") || text.contains("PRIVATE") {
        return Err("Private key or additional PEM objects refused".into());
    }
    let encoded: String = middle
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let der = STANDARD
        .decode(encoded)
        .map_err(|_| "Invalid public certificate encoding")?;
    if der.len() < 128 || der.first() != Some(&0x30) {
        return Err("Invalid public certificate DER envelope".into());
    }
    Ok(der)
}

pub fn public_certificate(path: &Path) -> Result<PublicCertificate> {
    let digest = Sha256::digest(certificate_der(&certificate_bytes(path)?)?);
    Ok(PublicCertificate {
        path: path.to_path_buf(),
        fingerprint: digest
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":"),
    })
}

enum CheckState {
    Preparing,
    Recording(CheckDetails),
    Finished,
}

pub struct SetupCapture {
    owner: CaptureOwner,
    tools: ToolSelection,
    serial: String,
    state: CheckState,
    started_at: String,
    offset: u64,
    pending: Vec<u8>,
    observed: bool,
}

impl SetupCapture {
    pub fn open(
        paths: CapturePaths,
        tools: ToolSelection,
        fixture: bool,
        serial: String,
    ) -> Result<Self> {
        Ok(Self {
            owner: CaptureOwner::open(paths, tools.clone(), fixture)?,
            tools,
            serial,
            state: CheckState::Preparing,
            started_at: now(),
            offset: 0,
            pending: vec![],
            observed: false,
        })
    }

    pub fn start(&mut self) -> Result<CheckDetails> {
        let request = StartRequest {
            serial: self.serial.clone(),
            whole_device_consent: false,
            selection: CaptureSelection::Network(NetworkConsent { whole_device: true }),
            limits: Limits::default(),
            log_scope: LogScope::WholeDevice,
            network_policy: NetworkPolicy {
                max_flows: 256,
                max_body_bytes: 1024 * 1024,
                max_header_bytes: 65536,
                max_total_bytes: 32 * 1024 * 1024,
                duration_ms: 45000,
            },
            context: CaptureContext {
                title: "Android device setup check".into(),
                notes: String::new(),
                steps: vec![],
            },
        };
        let snapshot = self.owner.start(request)?;
        let Snapshot::Recording(session) = snapshot else {
            return Err("Setup capture did not enter recording".into());
        };
        let NetworkView::Recording(coverage) = session.network else {
            return Err("Setup network source did not enter recording".into());
        };
        let url = coverage
            .synthetic_check_url
            .ok_or("Setup HTTPS URL is missing")?;
        check_nonce(&url)?;
        let certificate = public_certificate(
            &coverage
                .public_certificate
                .ok_or("Setup public certificate is missing")?,
        )?;
        self.started_at = session.started_at;
        let details = CheckDetails {
            directory: session.directory,
            url,
            certificate,
        };
        self.state = CheckState::Recording(details.clone());
        Ok(details)
    }

    pub fn directory(&self) -> Option<PathBuf> {
        self.owner.directory().map(Path::to_path_buf)
    }

    fn details(&self) -> Result<&CheckDetails> {
        match &self.state {
            CheckState::Recording(details) => Ok(details),
            _ => Err("No active setup check".into()),
        }
    }

    pub fn copy_public_certificate(&self) -> Result<String> {
        let details = self.details()?;
        let certificate = public_certificate(&details.certificate.path)?;
        if certificate.fingerprint != details.certificate.fingerprint {
            return Err("The capture certificate changed during preparation".into());
        }
        let fingerprint = certificate.fingerprint.replace(':', "").to_lowercase();
        let target = format!(
            "/sdcard/Download/mobile-capture-ca-{}.crt",
            &fingerprint[..12]
        );
        let mut stage = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
        use std::io::Write;
        stage
            .write_all(&certificate_bytes(&certificate.path)?)
            .map_err(|e| e.to_string())?;
        stage.flush().map_err(|e| e.to_string())?;
        crate::android::execute(
            &self.tools.adb,
            &self.serial,
            &[
                "push",
                stage
                    .path()
                    .to_str()
                    .ok_or("Certificate staging path is not UTF-8")?,
                &target,
            ],
        )?;
        Ok(target)
    }

    pub fn open_browser(&self) -> Result<()> {
        let url = &self.details()?.url;
        check_nonce(url)?;
        crate::android::execute(
            &self.tools.adb,
            &self.serial,
            &[
                "shell",
                "am",
                "start",
                "-a",
                "android.intent.action.VIEW",
                "-d",
                url,
            ],
        )?;
        Ok(())
    }

    pub fn observe_https(&mut self) -> Result<bool> {
        if self.observed {
            return Ok(true);
        }
        let details = self.details()?.clone();
        if public_certificate(&details.certificate.path)?.fingerprint
            != details.certificate.fingerprint
        {
            return Err("The capture certificate changed during the check".into());
        }
        let mut source = match files::open(&details.directory, "network/events.jsonl") {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.to_string()),
        };
        if !source.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Setup evidence is not a regular file".into());
        }
        source
            .seek(SeekFrom::Start(self.offset + self.pending.len() as u64))
            .map_err(|e| e.to_string())?;
        source
            .take(512 * 1024)
            .read_to_end(&mut self.pending)
            .map_err(|e| e.to_string())?;
        let mut consumed = 0;
        while let Some(end) = self.pending[consumed..].iter().position(|b| *b == b'\n') {
            let end = consumed + end + 1;
            if end - consumed > 5 * 1024 * 1024 {
                return Err("Setup evidence record exceeds its bounded check reader".into());
            }
            if let Ok(event) = serde_json::from_slice::<Value>(&self.pending[consumed..end]) {
                if valid_https_probe(&event, &details.url, &self.started_at)? {
                    self.observed = true;
                }
            }
            consumed = end;
        }
        self.pending.drain(..consumed);
        self.offset += consumed as u64;
        if self.pending.len() > 5 * 1024 * 1024 {
            return Err("Setup evidence record exceeds its bounded check reader".into());
        }
        Ok(self.observed)
    }

    pub fn stop(&mut self) -> Result<bool> {
        if self.owner.directory().is_none() {
            self.state = CheckState::Finished;
            return Ok(true);
        }
        let restored = restored(self.owner.stop()?);
        if restored {
            self.state = CheckState::Finished;
        }
        Ok(restored)
    }

    pub fn recover(&mut self) -> Result<bool> {
        let restored = restored(self.owner.recover()?);
        if restored {
            self.state = CheckState::Finished;
        }
        Ok(restored)
    }
}

fn restored(outcome: StopOutcome) -> bool {
    match outcome {
        StopOutcome::Ready(session) => matches!(
            session.network,
            NetworkView::Stopped(NetworkCompletion {
                routing_restored: true,
                ..
            }) | NetworkView::Interrupted(NetworkCompletion {
                routing_restored: true,
                ..
            })
        ),
        _ => false,
    }
}

fn check_nonce(url: &str) -> Result<&str> {
    let nonce = url
        .strip_prefix("https://mitm.it/mobile-capture-check/")
        .and_then(|s| s.strip_suffix("/https"))
        .ok_or("Setup requires the owned HTTPS endpoint")?;
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid owned setup nonce".into());
    }
    Ok(nonce)
}

pub fn valid_https_probe(event: &Value, url: &str, started_at: &str) -> Result<bool> {
    let nonce = check_nonce(url)?;
    let detail = &event["detail"];
    if event["source"] != "network"
        || event["kind"] != "response"
        || detail["status"] != 200
        || detail["url"] != url
        || detail["method"] != "GET"
        || detail["setup_probe"] != true
        || detail["setup_probe_nonce"] != nonce
        || detail["setup_probe_transport"] != "android_reverse"
    {
        return Ok(false);
    }
    let Some(received) = event["received_at"]
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    else {
        return Ok(false);
    };
    let started =
        DateTime::parse_from_rfc3339(started_at).map_err(|_| "Invalid setup start time")?;
    if received < started || received > Utc::now() + chrono::Duration::seconds(5) {
        return Ok(false);
    }
    let body = &detail["response_body"];
    if body["encoding"] != "base64"
        || body["byte_basis"] != "raw_content"
        || body["unavailable"] == true
        || body["preview"] == true
    {
        return Ok(false);
    }
    let Some(text) = body["text"].as_str().filter(|s| s.len() <= 8192) else {
        return Ok(false);
    };
    let Ok(bytes) = STANDARD.decode(text) else {
        return Ok(false);
    };
    if body["bytes"].as_u64() != Some(bytes.len() as u64) {
        return Ok(false);
    }
    let Ok(payload) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(false);
    };
    Ok(payload["nonce"] == nonce
        && payload["capture_check"] == "browser_https"
        && payload["message"] == "Synthetic capture check: café 日本語")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn probe() -> (Value, String, String) {
        let nonce = "12ab".repeat(8);
        let url = format!("https://mitm.it/mobile-capture-check/{nonce}/https");
        let start = now();
        let bytes = serde_json::to_vec(&json!({"nonce":nonce,"capture_check":"browser_https","message":"Synthetic capture check: café 日本語"})).unwrap();
        (
            json!({"source":"network","kind":"response","received_at":start,"detail":{
            "url":url,"method":"GET","status":200,"setup_probe":true,"setup_probe_nonce":nonce,
            "setup_probe_transport":"android_reverse","response_body":{"encoding":"base64","text":STANDARD.encode(&bytes),"bytes":bytes.len(),"byte_basis":"raw_content"}}}),
            url,
            start,
        )
    }

    #[test]
    fn only_complete_fresh_owned_https_response_is_observed() {
        let (event, url, start) = probe();
        assert!(valid_https_probe(&event, &url, &start).unwrap());
        for (path, value) in [
            ("status", json!(201)),
            ("url", json!(url.replace("https:", "http:"))),
            ("setup_probe_nonce", json!("wrong")),
            ("setup_probe_transport", json!("host")),
        ] {
            let mut altered = event.clone();
            altered["detail"][path] = value;
            assert!(!valid_https_probe(&altered, &url, &start).unwrap());
        }
        for (path, value) in [
            ("preview", json!(true)),
            ("unavailable", json!(true)),
            ("bytes", json!(1)),
            ("byte_basis", json!("content")),
        ] {
            let mut altered = event.clone();
            altered["detail"]["response_body"][path] = value;
            assert!(!valid_https_probe(&altered, &url, &start).unwrap());
        }
        let mut stale = event.clone();
        stale["received_at"] = json!("2000-01-01T00:00:00Z");
        assert!(!valid_https_probe(&stale, &url, &start).unwrap());
        assert!(valid_https_probe(&event, &url.replace("https:", "http:"), &start).is_err());
    }

    #[test]
    fn public_certificate_parser_refuses_private_or_multiple_pem_objects() {
        let der = vec![0x30; 256];
        let pem = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            STANDARD.encode(&der)
        );
        assert_eq!(certificate_der(pem.as_bytes()).unwrap(), der);
        assert!(certificate_der(
            format!("{pem}-----BEGIN PRIVATE KEY-----\nsecret\n-----END PRIVATE KEY-----")
                .as_bytes()
        )
        .is_err());
        assert!(certificate_der(format!("{pem}{pem}").as_bytes()).is_err());
        assert!(certificate_der(
            b"-----BEGIN CERTIFICATE-----\nZmFrZQ==\n-----END CERTIFICATE-----"
        )
        .is_err());
    }
}
