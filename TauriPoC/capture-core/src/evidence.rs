use crate::{files, model::*, timeline};
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
        directory, id, device, started_at, limits, receipt, fixture, None,
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
) -> Result<PathBuf> {
    let logs = match files::read(directory, "logs/device.log")? {
        Some(bytes) => bytes,
        None if network.is_some() => vec![],
        None => return Err("Raw logs are missing".into()),
    };
    if logs.len() as u64 != receipt.bytes {
        return Err("Raw log size does not match the stopped collector receipt".into());
    }
    let markers =
        files::read(directory, "timeline/markers.jsonl")?.ok_or("Raw markers are missing")?;
    let mut events = timeline::events(&logs, &markers, started_at);
    if network.is_some() {
        events.extend(timeline::network_events(directory, started_at, false)?);
        timeline::sort(&mut events);
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
    let mut paths = vec!["logs/device.log", "timeline/markers.jsonl"];
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
        if metadata.len() > 96 * 1024 * 1024 {
            return Err("Evidence exceeds enforced 96 MiB streaming bound".into());
        }
        let (bytes, hash) = digest(&mut file, 96 * 1024 * 1024)?;
        if path == "logs/device.log" && hash != timeline::hash(&logs)
            || path == "timeline/markers.jsonl" && hash != timeline::hash(&markers)
        {
            return Err("Raw source changed during normalization".into());
        }
        if path.starts_with("network/") {
            network_bytes += bytes;
            if network_bytes > 96 * 1024 * 1024 {
                return Err("Network artifacts exceed enforced 96 MiB export bound".into());
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
        || receipt.reason != StopReason::LeaseClosed
        || receipt.incomplete_tail
        || !receipt.stderr.is_empty()
        || receipt.forced
        || logs.is_empty()
        || !unparsed.is_empty()
        || !network_error_refs.is_empty()
        || !missing.is_empty();
    let public_network = network.map(|n| json!({"coverage":{"decoded_http_events":n.coverage.decoded_http_events,"tls_errors":n.coverage.tls_errors,"omitted_flows":n.coverage.omitted_flows,"unavailable_bodies":n.coverage.unavailable_bodies,"admission_closed":n.coverage.admission_closed},"routing_restored":n.routing_restored,"partial":n.partial,"forced":n.forced,"interrupted":n.interrupted,"reasons":n.reasons}));
    let context = json!({"log_scope":{"mode":"whole_device","explicit_consent":true,"app_scope_available":false},"device_logs":{"policy":limits,"persisted_bytes":receipt.bytes,"raw_records":receipt.records,"completion_reason":receipt.reason,"incomplete_tail":receipt.incomplete_tail},"network":public_network,"network_engine":if network.is_some(){json!({"name":"mitmproxy","version":"12.2.3","byte_basis":"raw_content","policy":incomplete["policy"]})}else{json!(null)},"coordinator":"tauri-poc","fixture":fixture});
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
            "Whole-device logs; app scope and network capture unavailable",
            "Device clock uncertainty unknown",
            if fixture {
                "FIXTURE MODE: compiled fake ADB evidence"
            } else {
                "Raw evidence requires review before sharing"
            },
        ]
    };
    let manifest = json!({"format":"device-capture-evidence","schema_version":1,"id":id,"platform":"android","device_id":device.serial,"device_name":device.name,"started_at":started_at,"stopped_at":receipt.ended_at,"phase":"stopped","capture_logs":true,"capture_network":network.is_some(),"sources":{"logs":if logs_count>0{"observed"}else{"started"},"network":network_status},"routing_restored":network.is_none_or(|n|n.routing_restored),"package_state":if partial{"partial"}else{"complete"},"counts":{"logs":logs_count,"network":network_count,"markers":marker_count},"warnings":warnings,"errors":if receipt.stderr.is_empty(){vec![]}else{vec![receipt.stderr.clone()]},"collector_status":receipt,"network_completion":public_network,"context":context,"clock_calibration":{"logs":{"offset_ms":null,"uncertainty_ms":null,"method":"not_available"},"network":{"offset_ms":0,"uncertainty_ms":if network.is_some(){json!(1)}else{json!(null)},"method":if network.is_some(){"local_host_clock"}else{"not_selected"}},"marker":{"offset_ms":0,"uncertainty_ms":1,"method":"local_host_clock"}},"completeness":{"partial":partial,"device_logs":{"coverage_ended_before_session_end":receipt.reason!=StopReason::LeaseClosed,"incomplete_tail":receipt.incomplete_tail},"sources":{"logs":{"requested":true,"events":logs_count,"observed":logs_count>0,"runtime_status":receipt.reason,"missing_artifacts":[],"empty_artifacts":if receipt.bytes==0{vec!["logs/device.log"]}else{vec![]}},"network":{"requested":network.is_some(),"events":network_count,"observed":decoded>0,"runtime_status":network_status,"missing_artifacts":missing,"empty_artifacts":[],"decoded_http_events":decoded}},"missing_artifacts":missing,"pending_flow_ids":incomplete["flow_ids"].as_array().cloned().unwrap_or_default(),"missing_bodies":incomplete["missing_bodies"].as_array().cloned().unwrap_or_default(),"omitted_flows":incomplete["omitted_flows"].as_u64().unwrap_or(0),"network_error_refs":network_error_refs,"unparsed_event_refs":unparsed,"unknown_clock_events":unknown},"artifacts":artifacts,"correlation":{"time_proximity":"inferred","causal_matching_requires_shared_identifiers":true}});
    let stage = tempfile::Builder::new()
        .prefix(".export-")
        .tempdir_in(directory)
        .map_err(|e| e.to_string())?;
    let mut timeline_file =
        tempfile::NamedTempFile::new_in(stage.path()).map_err(|e| e.to_string())?;
    for event in &events {
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
        serde_json::to_writer(&mut timeline_file, event).map_err(|e| e.to_string())?;
        timeline_file.write_all(b"\n").map_err(|e| e.to_string())?;
    }
    timeline_file.flush().map_err(|e| e.to_string())?;
    let (_, hash) = digest(timeline_file.as_file_mut(), 512 * 1024 * 1024)?;
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
    generated.insert("README.txt".to_string(),format!("Mobile Capture Tauri PoC, schema 1\nWhole-device Android logs, explicitly consented. {}\nRead manifest.json for gaps, limits, fixture provenance, and shutdown outcomes. ZIP creation does not establish completeness.\nRaw evidence is unchanged. Original HTTP content-encoding bytes use raw_content basis. Device clock uncertainty is unknown. Timestamp proximity is inferred.\nSHA256SUMS covers every included file except itself. Private runtime, tokens, journal, spools, and CA keys are excluded.\n",if network.is_some(){"HTTP(S) proxy evidence requires manual CA trust. Hard guardian death may interrupt its listener. Routing restoration and original writer quiescence were verified."}else{"App scope and network capture are unavailable. No routing or trust settings were changed."}).into_bytes());
    for (path, bytes) in &generated {
        checksums.insert(path.clone(), timeline::hash(bytes));
    }
    generated.insert(
        "SHA256SUMS".into(),
        checksums
            .iter()
            .map(|(p, h)| format!("{h}  {p}\n"))
            .collect::<String>()
            .into_bytes(),
    );
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
        archive.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    for (path, mut source) in raw {
        archive
            .start_file(&path, options)
            .map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut streamed = 0u64;
        let max = if path == "timeline/events.jsonl" {
            512 * 1024 * 1024
        } else {
            96 * 1024 * 1024
        };
        let mut buffer = [0; 65536];
        loop {
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
    publish(&temporary, &target)?;
    Ok(target)
}
fn digest(file: &mut fs::File, limit: u64) -> Result<(u64, String)> {
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut bytes = [0; 65536];
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    loop {
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
#[cfg(not(windows))]
fn publish(source: &Path, target: &Path) -> Result<()> {
    fs::rename(source, target).map_err(|e| e.to_string())
}
#[cfg(windows)]
fn publish(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let from: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}
