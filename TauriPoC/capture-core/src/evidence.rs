use crate::{
    files,
    model::*,
    save::{self, SaveHandle, Stage},
    timeline,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

pub fn export(
    directory: &Path,
    id: &str,
    device: &Device,
    started_at: &str,
    limits: &Limits,
    receipt: &Receipt,
    fixture: bool,
) -> Result<PathBuf> {
    export_impl(
        directory, id, device, started_at, limits, receipt, fixture, None, None,
    )
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn export_network(
    directory: &Path,
    id: &str,
    device: &Device,
    started_at: &str,
    limits: &Limits,
    receipt: &Receipt,
    fixture: bool,
    sealed: &crate::network::SealedEvidence,
) -> Result<PathBuf> {
    export_impl(
        directory,
        id,
        device,
        started_at,
        limits,
        receipt,
        fixture,
        Some(sealed.completion()),
        None,
    )
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn export_controlled(
    directory: &Path,
    id: &str,
    device: &Device,
    started_at: &str,
    limits: &Limits,
    receipt: &Receipt,
    fixture: bool,
    network: Option<&crate::network::SealedEvidence>,
    control: &SaveHandle,
) -> Result<PathBuf> {
    control.matches(directory)?;
    export_impl(
        directory,
        id,
        device,
        started_at,
        limits,
        receipt,
        fixture,
        network.map(|n| n.completion()),
        Some(control),
    )
}
#[allow(clippy::too_many_arguments)]
fn export_impl(
    directory: &Path,
    id: &str,
    device: &Device,
    started_at: &str,
    limits: &Limits,
    receipt: &Receipt,
    fixture: bool,
    network: Option<&NetworkCompletion>,
    control: Option<&SaveHandle>,
) -> Result<PathBuf> {
    save::update(control, Stage::Validate, 0, None)?;
    let descriptor_bytes = save::read(directory, "capture.json", control)?;
    let descriptor: Option<SessionDescriptor> = descriptor_bytes
        .as_ref()
        .map(|bytes| serde_json::from_slice(bytes).map_err(|e| e.to_string()))
        .transpose()?;
    let logs_selected = descriptor
        .as_ref()
        .is_none_or(|d| d.selection.includes_logs());
    let logs = match save::read(directory, "logs/device.log", control)? {
        Some(bytes) => bytes,
        None if network.is_some() => vec![],
        None => return Err("Raw logs are missing".into()),
    };
    if logs.len() as u64 != receipt.bytes {
        return Err("Raw log size does not match the stopped collector receipt".into());
    }
    let markers = save::read(directory, "timeline/markers.jsonl", control)?
        .ok_or("Raw markers are missing")?;
    save::update(control, Stage::Normalize, 0, None)?;
    let mut events = timeline::events_controlled(&logs, &markers, started_at, control)?;
    if network.is_some() {
        events.extend(timeline::network_events_controlled(
            directory, started_at, false, control,
        )?);
        timeline::sort_controlled(&mut events, control)?;
    }
    let logs_count = events.iter().filter(|e| e.source == "logs").count();
    let network_count = events.iter().filter(|e| e.source == "network").count();
    let marker_count = events.iter().filter(|e| e.source == "marker").count();
    let decoded = events
        .iter()
        .filter(|e| e.source == "network" && matches!(e.kind.as_str(), "request" | "response"))
        .count();
    let unparsed: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind.as_str(), "unparsed_log" | "parse_error"))
        .map(|e| e.raw_ref.clone())
        .collect();
    let network_error_refs: Vec<_> = events
        .iter()
        .filter(|e| e.source == "network" && matches!(e.kind.as_str(), "error" | "tls_error"))
        .map(|e| e.raw_ref.clone())
        .collect();
    let unknown = events
        .iter()
        .filter(|e| e.clock_uncertainty_ms.is_none())
        .count();
    let mut raw = BTreeMap::new();
    let mut artifacts = vec![];
    let mut checksums = BTreeMap::new();
    let mut missing = vec![];
    let mut network_bytes = 0;
    let mut paths = vec!["timeline/markers.jsonl"];
    if logs_selected {
        paths.push("logs/device.log");
    }
    if descriptor.is_some() {
        paths.push("capture.json");
    }
    if network.is_some() {
        paths.extend([
            "network/events.jsonl",
            "network/capture.mitm",
            "network/capture.har",
            "network/incomplete.json",
        ]);
    }
    for path in paths {
        let mut file = match files::open(directory, path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(path);
                continue;
            }
            Err(e) => return Err(format!("Refused evidence path {path}: {e}")),
        };
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            return Err(format!("Evidence is not regular {path}"));
        }
        if metadata.len() > 1024 * 1024 * 1024 {
            return Err("Evidence exceeds enforced 1 GiB streaming bound".into());
        }
        let (bytes, hash) = digest(&mut file, 1024 * 1024 * 1024, control)?;
        if path == "logs/device.log" && hash != save::hash(&logs, control)?
            || path == "timeline/markers.jsonl" && hash != save::hash(&markers, control)?
        {
            return Err("Raw source changed during normalization".into());
        }
        if path.starts_with("network/") {
            network_bytes += bytes;
            if network_bytes > 1024 * 1024 * 1024 {
                return Err("Network artifacts exceed enforced 1 GiB export bound".into());
            }
        }
        artifacts.push(json!({"path":path,"bytes":bytes,"sha256":hash}));
        checksums.insert(path.to_string(), hash);
        raw.insert(path.to_string(), file);
    }
    let incomplete = if network.is_some() {
        crate::lease::read::<serde_json::Value>(&directory.join("network/incomplete.json"))
            .unwrap_or(json!({}))
    } else {
        json!({})
    };
    let partial = network.is_some_and(|n| n.partial)
        || logs_selected
            && (receipt.reason != StopReason::LeaseClosed
                || receipt.incomplete_tail
                || !receipt.stderr.is_empty()
                || receipt.forced
                || logs.is_empty())
        || !unparsed.is_empty()
        || !network_error_refs.is_empty()
        || !missing.is_empty();
    let public_network = network.map(|n| json!({"coverage":{"decoded_http_events":n.coverage.decoded_http_events,"tls_errors":n.coverage.tls_errors,"omitted_flows":n.coverage.omitted_flows,"unavailable_bodies":n.coverage.unavailable_bodies,"admission_closed":n.coverage.admission_closed},"routing_restored":n.routing_restored,"partial":n.partial,"forced":n.forced,"interrupted":n.interrupted,"reasons":n.reasons}));
    let context = json!({"log_scope": descriptor.as_ref().map(|d|serde_json::to_value(&d.log_scope).unwrap()).unwrap_or(json!({"mode":"whole_device","legacy_metadata":true})),"log_attribution":descriptor.as_ref().and_then(|d|d.log_attribution.clone()),"capture_context":descriptor.as_ref().map(|d|d.context.clone()).unwrap_or_default(),"device_logs":{"requested":logs_selected,"policy":limits,"persisted_bytes":receipt.bytes,"raw_records":receipt.records,"completion_reason":if logs_selected{json!(receipt.reason)}else{json!("not_selected")},"incomplete_tail":receipt.incomplete_tail},"network":public_network,"network_engine":if network.is_some(){json!({"name":"mitmproxy","version":"12.2.3","byte_basis":"raw_content","policy":descriptor.as_ref().map(|d|serde_json::to_value(&d.network_policy).unwrap()).unwrap_or(incomplete["policy"].clone())})}else{json!(null)},"coordinator":"mobile-capture-android","fixture":fixture});
    let network_status = if network.is_none() {
        "not_selected"
    } else if decoded > 0 {
        "observed"
    } else if network.is_some_and(|n| n.coverage.tls_errors > 0) {
        "tls_failed"
    } else {
        "started"
    };
    let warnings = if network.is_some() {
        vec!["Whole-device proxy evidence. Manual CA trust, certificate pinning, and omissions limit coverage.","Guardian hard death may interrupt the listener and original evidence.",if fixture{"FIXTURE MODE: compiled fake ADB evidence"}else{"Device clock uncertainty unknown"}]
    } else {
        vec![
            "Android logs scoped as recorded in capture.json and context.json",
            "Device clock uncertainty unknown",
            if fixture {
                "FIXTURE MODE: compiled fake ADB evidence"
            } else {
                "Raw evidence requires review before sharing"
            },
        ]
    };
    let manifest = json!({"format":"device-capture-evidence","schema_version":1,"id":id,"platform":"android","device_id":device.serial,"device_name":device.name,"started_at":started_at,"stopped_at":receipt.ended_at,"phase":"stopped","capture_logs":logs_selected,"capture_network":network.is_some(),"sources":{"logs":if !logs_selected{"not_selected"}else if logs_count>0{"observed"}else{"started"},"network":network_status},"routing_restored":network.is_none_or(|n|n.routing_restored),"package_state":if partial{"partial"}else{"complete"},"counts":{"logs":logs_count,"network":network_count,"markers":marker_count},"warnings":warnings,"errors":if receipt.stderr.is_empty(){vec![]}else{vec![receipt.stderr.clone()]},"collector_status":if logs_selected{json!(receipt)}else{json!(null)},"network_completion":public_network,"context":context,"clock_calibration":{"logs":{"offset_ms":null,"uncertainty_ms":null,"method":"not_available"},"network":{"offset_ms":0,"uncertainty_ms":if network.is_some(){json!(1)}else{json!(null)},"method":if network.is_some(){"local_host_clock"}else{"not_selected"}},"marker":{"offset_ms":0,"uncertainty_ms":1,"method":"local_host_clock"}},"completeness":{"partial":partial,"device_logs":{"coverage_ended_before_session_end":logs_selected && receipt.reason!=StopReason::LeaseClosed,"incomplete_tail":receipt.incomplete_tail},"sources":{"logs":{"requested":logs_selected,"events":logs_count,"observed":logs_count>0,"runtime_status":if logs_selected{json!(receipt.reason)}else{json!("not_selected")},"missing_artifacts":[],"empty_artifacts":if logs_selected && receipt.bytes==0{vec!["logs/device.log"]}else{vec![]}},"network":{"requested":network.is_some(),"events":network_count,"observed":decoded>0,"runtime_status":network_status,"missing_artifacts":missing,"empty_artifacts":[],"decoded_http_events":decoded}},"missing_artifacts":missing,"pending_flow_ids":incomplete["flow_ids"].as_array().cloned().unwrap_or_default(),"missing_bodies":incomplete["missing_bodies"].as_array().cloned().unwrap_or_default(),"omitted_flows":incomplete["omitted_flows"].as_u64().unwrap_or(0),"network_error_refs":network_error_refs,"unparsed_event_refs":unparsed,"unknown_clock_events":unknown},"artifacts":artifacts,"correlation":{"time_proximity":"inferred","causal_matching_requires_shared_identifiers":true}});
    let stage = tempfile::Builder::new()
        .prefix(".export-")
        .tempdir_in(directory)
        .map_err(|e| e.to_string())?;
    let mut timeline_file =
        tempfile::NamedTempFile::new_in(stage.path()).map_err(|e| e.to_string())?;
    for (index, event) in events.iter().enumerate() {
        if index % 128 == 0 {
            save::update(
                control,
                Stage::Normalize,
                index as u64,
                Some(events.len() as u64),
            )?;
        }
        let (path, line) = event
            .raw_ref
            .rsplit_once("#L")
            .ok_or("Canonical event has no line reference")?;
        let line: usize = line
            .parse()
            .map_err(|_| "Invalid canonical line reference")?;
        if line == 0 || !raw.contains_key(path) {
            return Err("Canonical event refers to absent raw evidence".into());
        }
        let mut writer = std::io::BufWriter::with_capacity(
            save::CHUNK,
            CheckedWriter::new(&mut timeline_file, control),
        );
        serde_json::to_writer(&mut writer, event).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        drop(writer);
        timeline_file.write_all(b"\n").map_err(|e| e.to_string())?;
    }
    timeline_file.flush().map_err(|e| e.to_string())?;
    let (_, hash) = digest(timeline_file.as_file_mut(), 2 * 1024 * 1024 * 1024, control)?;
    checksums.insert("timeline/events.jsonl".into(), hash);
    raw.insert(
        "timeline/events.jsonl".into(),
        timeline_file.reopen().map_err(|e| e.to_string())?,
    );
    let mut generated = BTreeMap::new();
    generated.insert(
        "manifest.json".to_string(),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    );
    generated.insert("context.json".to_string(),serde_json::to_vec_pretty(&json!({"id":id,"platform":"android","device_id":device.serial,"device_name":device.name,"started_at":started_at,"stopped_at":receipt.ended_at,"context":context})).map_err(|e|e.to_string())?);
    generated.insert("README.txt".to_string(),format!("Mobile Capture Android, schema 1\nSource selection and app attribution are recorded in capture.json and context.json. {}\nRead manifest.json for gaps, limits, fixture provenance, and shutdown outcomes. ZIP creation does not establish completeness.\nRaw evidence is unchanged. Original HTTP content-encoding bytes use raw_content basis. Device clock uncertainty is unknown. Timestamp proximity is inferred.\nSHA256SUMS covers every included file except itself. Private runtime, tokens, journal, spools, and CA keys are excluded.\n",if network.is_some(){"HTTP(S) proxy evidence requires manual CA trust. Hard guardian death may interrupt its listener. Routing restoration and original writer quiescence were verified."}else{"Log-only recording. No routing or trust settings were changed."}).into_bytes());
    for (path, bytes) in &generated {
        checksums.insert(path.clone(), save::hash(bytes, control)?);
    }
    generated.insert(
        "SHA256SUMS".into(),
        checksums
            .iter()
            .map(|(p, h)| format!("{h}  {p}\n"))
            .collect::<String>()
            .into_bytes(),
    );
    let total = raw
        .values()
        .try_fold(0u64, |total, file| file.metadata().map(|m| total + m.len()))
        .map_err(|e| e.to_string())?
        + generated.values().map(|b| b.len() as u64).sum::<u64>();
    let mut completed = 0u64;
    save::update(control, Stage::Archive, completed, Some(total))?;
    let temporary = stage.path().join("capture-package.zip");
    let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o600);
    let mut archive = ZipWriter::new(&mut file);
    for (path, bytes) in generated {
        archive
            .start_file(path, options)
            .map_err(|e| e.to_string())?;
        for chunk in bytes.chunks(save::CHUNK) {
            save::check(control)?;
            archive.write_all(chunk).map_err(|e| e.to_string())?;
            completed += chunk.len() as u64;
            save::update(control, Stage::Archive, completed, Some(total))?;
        }
    }
    for (path, mut source) in raw {
        archive
            .start_file(&path, options)
            .map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut streamed = 0u64;
        let max = if path == "timeline/events.jsonl" {
            2 * 1024 * 1024 * 1024
        } else {
            1024 * 1024 * 1024
        };
        let mut buffer = [0; 65536];
        loop {
            save::check(control)?;
            let n = source.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            streamed += n as u64;
            if streamed > max {
                return Err("Sealed source grew beyond its streaming export bound".into());
            }
            hasher.update(&buffer[..n]);
            archive.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            completed += n as u64;
            save::update(control, Stage::Archive, completed, Some(total))?;
        }
        if format!("{:x}", hasher.finalize()) != checksums[&path] {
            return Err("Sealed evidence changed during export".into());
        }
    }
    archive.finish().map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    let target = directory.join("capture-package.zip");
    files::refuse_link(&target)?;
    match control {
        Some(control) => control.publish(&temporary, &target)?,
        None => save::publish(&temporary, &target)?,
    }
    Ok(target)
}
fn digest(file: &mut fs::File, limit: u64, control: Option<&SaveHandle>) -> Result<(u64, String)> {
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut bytes = [0; 65536];
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    loop {
        save::check(control)?;
        let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
        if count + n as u64 > limit {
            return Err(format!(
                "Source grew beyond enforced streaming bound of {limit} bytes"
            ));
        }
        if n == 0 {
            break;
        }
        count += n as u64;
        hash.update(&bytes[..n]);
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    Ok((count, format!("{:x}", hash.finalize())))
}
struct CheckedWriter<'a, W> {
    inner: W,
    control: Option<&'a SaveHandle>,
}
impl<'a, W> CheckedWriter<'a, W> {
    fn new(inner: W, control: Option<&'a SaveHandle>) -> Self {
        Self { inner, control }
    }
}
impl<W: Write> Write for CheckedWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        save::check(self.control).map_err(std::io::Error::other)?;
        self.inner.write(&bytes[..bytes.len().min(save::CHUNK)])
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn cancellation_during_real_zip_write_preserves_raw_and_prior_zip_then_retry_succeeds() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("logs")).unwrap();
        fs::create_dir(root.path().join("timeline")).unwrap();
        let logs = vec![b'x'; 8 * 1024 * 1024];
        fs::write(root.path().join("logs/device.log"), &logs).unwrap();
        fs::write(root.path().join("timeline/markers.jsonl"), b"").unwrap();
        let target = root.path().join("capture-package.zip");
        fs::write(&target, b"prior archive").unwrap();
        let receipt = Receipt {
            reason: StopReason::LeaseClosed,
            bytes: logs.len() as u64,
            records: 1,
            incomplete_tail: true,
            child_reaped: true,
            forced: false,
            exit_code: Some(0),
            stderr: String::new(),
            ended_at: "2026-10-20T00:00:01Z".into(),
        };
        let device = Device {
            serial: "fixture".into(),
            name: "fixture".into(),
            state: DeviceState::Ready,
        };
        let handle = SaveHandle::create(root.path()).unwrap();
        let cancel = handle.clone();
        let directory = root.path().to_path_buf();
        let receipt_copy = receipt.clone();
        let device_copy = device.clone();
        let worker = thread::spawn(move || {
            handle.run(|| {
                export_controlled(
                    &directory,
                    "fixture",
                    &device_copy,
                    "2026-10-20T00:00:00Z",
                    &Limits::default(),
                    &receipt_copy,
                    true,
                    None,
                    &handle,
                )
            })
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let progress = cancel.progress().unwrap();
            if progress.stage == "archive" && progress.completed > 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "never observed archive progress {progress:?}"
            );
            assert!(
                !worker.is_finished(),
                "archive completed before cancellation was exercised"
            );
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(cancel.cancel().unwrap(), save::CancelOutcome::Accepted);
        assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
        assert_eq!(fs::read(&target).unwrap(), b"prior archive");
        assert_eq!(fs::read(root.path().join("logs/device.log")).unwrap(), logs);
        assert!(!fs::read_dir(root.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".export-")));
        let retry = SaveHandle::create(root.path()).unwrap();
        retry
            .run(|| {
                export_controlled(
                    root.path(),
                    "fixture",
                    &device,
                    "2026-10-20T00:00:00Z",
                    &Limits::default(),
                    &receipt,
                    true,
                    None,
                    &retry,
                )
            })
            .unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(&target).unwrap()).unwrap();
        let mut archived_logs = Vec::new();
        zip.by_name("logs/device.log")
            .unwrap()
            .read_to_end(&mut archived_logs)
            .unwrap();
        assert_eq!(archived_logs, logs);
        assert!(zip
            .file_names()
            .all(|name| !name.starts_with("runtime/") && !name.contains("state.json")));
        assert_eq!(retry.progress().unwrap().stage, "complete");
    }
}
