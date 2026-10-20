use crate::{files, model::*, timeline};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::{Seek, Write},
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
    let mut entries = BTreeMap::new();
    let logs = files::read(directory, "logs/device.log")?.ok_or("Raw logs are missing")?;
    if logs.len() as u64 != receipt.bytes {
        return Err("Raw log size does not match the stopped collector receipt".into());
    }
    let markers = files::read(directory, "timeline/markers.jsonl")?.unwrap_or_default();
    let events = timeline::events(&logs, &markers, started_at);
    let logs_count = events.iter().filter(|e| e.source == "logs").count();
    let unparsed: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "unparsed_log" || e.kind == "parse_error")
        .map(|e| e.raw_ref.clone())
        .collect();
    let unknown = events
        .iter()
        .filter(|e| e.clock_uncertainty_ms.is_none())
        .count();
    let partial = receipt.reason != StopReason::LeaseClosed
        || receipt.incomplete_tail
        || !receipt.stderr.is_empty()
        || receipt.forced
        || logs.is_empty()
        || !unparsed.is_empty();
    let context = json!({"log_scope": {"mode": "whole_device", "explicit_consent": true, "app_scope_available": false}, "device_logs": {"policy": limits, "persisted_bytes": receipt.bytes, "raw_records": receipt.records, "completion_reason": receipt.reason, "incomplete_tail": receipt.incomplete_tail}, "coordinator": "tauri-poc", "fixture": fixture});
    entries.insert("logs/device.log".to_string(), logs);
    if !markers.is_empty() {
        entries.insert("timeline/markers.jsonl".to_string(), markers);
    }
    let artifacts: Vec<_> = entries.iter().map(|(path, bytes)| json!({"path": path, "bytes": bytes.len(), "sha256": timeline::hash(bytes)})).collect();
    let warnings = if fixture {
        vec![
            "FIXTURE MODE: compiled fake ADB evidence",
            "Whole-device logs; app scope and network capture unavailable",
            "Device clock uncertainty unknown",
        ]
    } else {
        vec![
            "Whole-device logs; app scope and network capture unavailable",
            "Device clock uncertainty unknown",
        ]
    };
    let manifest = json!({"format": "device-capture-evidence", "schema_version": 1, "id": id, "platform": "android", "device_id": device.serial, "device_name": device.name, "started_at": started_at, "stopped_at": receipt.ended_at, "phase": "stopped", "capture_logs": true, "capture_network": false, "sources": {"logs": if logs_count > 0 { "observed" } else { "started" }, "network": "not_selected"}, "routing_restored": true, "package_state": if partial { "partial" } else { "complete" }, "counts": {"logs": logs_count, "network": 0, "markers": events.len() - logs_count}, "warnings": warnings, "errors": if receipt.stderr.is_empty() { vec![] } else { vec![receipt.stderr.clone()] }, "collector_status": receipt, "context": context, "clock_calibration": {"logs": {"offset_ms": null, "uncertainty_ms": null, "method": "not_available"}, "network": {"offset_ms": null, "uncertainty_ms": null, "method": "not_selected"}, "marker": {"offset_ms": 0, "uncertainty_ms": 1, "method": "local_host_clock"}}, "completeness": {"partial": partial, "device_logs": {"coverage_ended_before_session_end": receipt.reason != StopReason::LeaseClosed, "incomplete_tail": receipt.incomplete_tail}, "sources": {"logs": {"requested": true, "events": logs_count, "observed": logs_count > 0, "runtime_status": receipt.reason, "missing_artifacts": [], "empty_artifacts": if receipt.bytes == 0 { vec!["logs/device.log"] } else { vec![] }}, "network": {"requested": false, "events": 0, "observed": false, "runtime_status": "not_selected", "missing_artifacts": [], "empty_artifacts": [], "decoded_http_events": 0}}, "missing_artifacts": [], "pending_flow_ids": [], "network_error_refs": [], "unparsed_event_refs": unparsed, "unknown_clock_events": unknown}, "artifacts": artifacts, "correlation": {"time_proximity": "inferred", "causal_matching_requires_shared_identifiers": true}});
    let mut timeline_bytes = Vec::new();
    for event in events {
        serde_json::to_writer(&mut timeline_bytes, &event).map_err(|e| e.to_string())?;
        timeline_bytes.push(b'\n');
    }
    entries.insert("timeline/events.jsonl".into(), timeline_bytes);
    entries.insert(
        "manifest.json".into(),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    );
    entries.insert("context.json".into(), serde_json::to_vec_pretty(&json!({"id": id, "platform": "android", "device_id": device.serial, "device_name": device.name, "started_at": started_at, "stopped_at": receipt.ended_at, "context": context})).map_err(|e| e.to_string())?);
    entries.insert("README.txt".into(), b"Mobile Capture Tauri PoC, schema 1\nWhole-device Android logs, explicitly consented. App scope and network capture are unavailable.\nRead manifest.json for gaps, limits, fixture provenance, and shutdown outcomes. ZIP creation does not establish completeness.\nRaw evidence is unchanged. Device clock uncertainty is unknown. Timestamp proximity is inferred.\nNo routing or trust settings were changed. Review raw evidence before sharing.\nSHA256SUMS covers each included file except itself. Private runtime state is excluded.\n".to_vec());
    let checksums = entries
        .iter()
        .map(|(path, bytes)| format!("{}  {path}\n", timeline::hash(bytes)))
        .collect::<String>();
    entries.insert("SHA256SUMS".into(), checksums.into_bytes());
    let stage = tempfile::Builder::new()
        .prefix(".export-")
        .tempdir_in(directory)
        .map_err(|e| e.to_string())?;
    let temporary = stage.path().join("capture-package.zip");
    let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
    write_zip(&mut file, entries)?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    let target = directory.join("capture-package.zip");
    files::refuse_link(&target)?;
    publish(&temporary, &target)?;
    Ok(target)
}
fn write_zip(file: &mut (impl Write + Seek), entries: BTreeMap<String, Vec<u8>>) -> Result<()> {
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o600);
    for (path, bytes) in entries {
        archive
            .start_file(path, options)
            .map_err(|e| e.to_string())?;
        archive.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    archive.finish().map_err(|e| e.to_string())?;
    Ok(())
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
