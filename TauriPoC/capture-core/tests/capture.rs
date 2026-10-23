use capture_poc_core::{model::*, CaptureOwner};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn owner(root: &Path) -> CaptureOwner {
    owner_with_adb(root, env!("CARGO_BIN_EXE_fake-adb").into())
}
fn owner_with_adb(root: &Path, adb: PathBuf) -> CaptureOwner {
    CaptureOwner::open(
        CapturePaths {
            output_root: root.into(),
            runtime_root: Some(root.join("fixture-runtime")),
        },
        ToolSelection {
            adb,
            worker: env!("CARGO_BIN_EXE_collector-worker").into(),
            guardian: PathBuf::new(),
            mitmdump: None,
            addon: PathBuf::new(),
        },
        true,
    )
    .unwrap()
}
fn view(owner: &mut CaptureOwner) -> Value {
    serde_json::to_value(owner.snapshot().unwrap()).unwrap()
}
fn wait(owner: &mut CaptureOwner, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let value = view(owner);
        if predicate(&value) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "Capture did not reach expected state: {value}"
        );
        thread::sleep(Duration::from_millis(40));
    }
}
fn start(owner: &mut CaptureOwner, serial: &str, limits: Limits) {
    owner
        .start(StartRequest {
            serial: serial.into(),
            whole_device_consent: true,
            selection: CaptureSelection::Logs,
            limits,
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        })
        .unwrap();
}
fn zip_entries(path: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|index| {
            let mut entry = zip.by_index(index).unwrap();
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}
#[test]
fn consent_is_required_and_unready_devices_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    assert!(owner
        .start(StartRequest {
            serial: "fixture-stream".into(),
            whole_device_consent: false,
            selection: CaptureSelection::Logs,
            limits: Limits::default(),
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        })
        .unwrap_err()
        .contains("consent"));
    assert!(owner
        .start(StartRequest {
            serial: "fixture-offline".into(),
            whole_device_consent: true,
            selection: CaptureSelection::Logs,
            limits: Limits::default(),
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        })
        .is_err());
    assert!(owner
        .start(StartRequest {
            serial: "fixture-denied".into(),
            whole_device_consent: true,
            selection: CaptureSelection::Logs,
            limits: Limits::default(),
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        })
        .is_err());
    assert!(owner.directory().is_none());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}
#[test]
fn raw_bytes_live_ids_markers_stop_and_export_are_real() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    start(&mut owner, "fixture-stream", Limits::default());
    assert!(owner.export().is_err());
    let live = wait(&mut owner, |v| {
        v["session"]["logs"].as_u64().unwrap_or(0) >= 4
    });
    let first = live["session"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["raw_ref"] == "logs/device.log#L1")
        .unwrap()
        .clone();
    let text = "quote \" slash \\ newline\nUnicode ✓";
    owner.marker(text.into()).unwrap();
    let stopped = serde_json::to_value(owner.stop().unwrap()).unwrap();
    assert_eq!(stopped["outcome"], "ready");
    assert_eq!(stopped["session"]["receipt"]["child_reaped"], true);
    assert_eq!(
        serde_json::to_value(owner.stop().unwrap()).unwrap(),
        stopped
    );
    assert!(owner.marker("late".into()).is_err());
    let raw = fs::read(owner.directory().unwrap().join("logs/device.log")).unwrap();
    assert!(raw.starts_with(b"1792022401.125  123  456 I Fixture: first raw record\r\n\n"));
    assert!(raw.contains(&255));
    let archive = owner.export().unwrap();
    let entries = zip_entries(&archive);
    assert_eq!(entries["logs/device.log"], raw);
    assert!(!entries.contains_key("session.json"));
    assert!(!entries.contains_key("receipt.json"));
    let events: Vec<Value> = entries["timeline/events.jsonl"]
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    let exported_first = events
        .iter()
        .find(|e| e["raw_ref"] == "logs/device.log#L1")
        .unwrap();
    assert_eq!(first, *exported_first);
    assert_eq!(
        events.iter().find(|e| e["source"] == "marker").unwrap()["summary"],
        text
    );
    assert_eq!(
        entries["timeline/markers.jsonl"]
            .iter()
            .filter(|b| **b == b'\n')
            .count(),
        1
    );
    for line in String::from_utf8(entries["SHA256SUMS"].clone())
        .unwrap()
        .lines()
    {
        let (hash, path) = line.split_once("  ").unwrap();
        assert_eq!(hash, format!("{:x}", Sha256::digest(&entries[path])));
    }
    let original_zip = fs::read(&archive).unwrap();
    owner.export().unwrap();
    assert_eq!(original_zip, fs::read(&archive).unwrap());
    let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["sources"]["network"], "not_selected");
    assert_eq!(manifest["package_state"], "partial");
    let marker_raw = entries["timeline/markers.jsonl"]
        .split(|b| *b == b'\n')
        .next()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(marker_raw).unwrap()["text"],
        text
    );
}
#[test]
fn disconnect_retains_incomplete_evidence_and_truthful_outcome() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    start(&mut owner, "fixture-disconnect", Limits::default());
    let state = wait(&mut owner, |v| v["phase"] == "failed");
    assert_eq!(state["session"]["receipt"]["incomplete_tail"], true);
    let entries = zip_entries(&owner.export().unwrap());
    assert!(entries["logs/device.log"].ends_with(b"incomplete tail"));
    let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).unwrap();
    assert_eq!(manifest["package_state"], "partial");
    assert_eq!(manifest["collector_status"]["reason"], "collector_exit");
}
#[test]
fn bounds_end_admission_without_evicting_earlier_bytes() {
    for (limits, reason) in [
        (
            Limits {
                bytes: 200,
                ..Limits::default()
            },
            "byte_limit",
        ),
        (
            Limits {
                records: 6,
                ..Limits::default()
            },
            "record_limit",
        ),
        (
            Limits {
                duration_ms: 120,
                ..Limits::default()
            },
            "duration_limit",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut owner = owner(root.path());
        let max = limits.clone();
        start(
            &mut owner,
            if reason == "duration_limit" {
                "fixture-stream"
            } else {
                "fixture-flood"
            },
            limits,
        );
        let state = wait(&mut owner, |v| v["phase"] == "stopped");
        assert_eq!(state["session"]["receipt"]["reason"], reason);
        let bytes = fs::read(owner.directory().unwrap().join("logs/device.log")).unwrap();
        assert!(bytes.len() as u64 <= max.bytes);
        assert!(bytes.iter().filter(|b| **b == b'\n').count() as u64 <= max.records);
        assert!(bytes.starts_with(b"1792022401.125"));
        owner.export().unwrap();
    }
}
#[test]
fn startup_requires_a_newline_terminated_config_before_collection() {
    for frame in ["empty", "json_without_newline", "partial_json"] {
        let root = tempfile::tempdir().unwrap();
        let config = WorkerConfig {
            source_lease: false,
            adb: env!("CARGO_BIN_EXE_fake-adb").into(),
            serial: "fixture-stream".into(),
            directory: root.path().into(),
            limits: Limits::default(),
            log_scope: LogScope::default(),
            resolved_logs: None,
        };
        let json = serde_json::to_vec(&config).unwrap();
        let bytes = match frame {
            "empty" => &json[..0],
            "partial_json" => &json[..json.len() / 2],
            _ => &json,
        };
        let mut child = Command::new(env!("CARGO_BIN_EXE_collector-worker"))
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        assert!(child.wait().unwrap().success(), "{frame}");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0, "{frame}");
    }
}
#[test]
fn parent_eof_stops_the_collector_after_a_complete_startup_frame() {
    let root = tempfile::tempdir().unwrap();
    let config = WorkerConfig {
        source_lease: false,
        adb: env!("CARGO_BIN_EXE_fake-adb").into(),
        serial: "fixture-stream".into(),
        directory: root.path().into(),
        limits: Limits::default(),
        log_scope: LogScope::default(),
        resolved_logs: None,
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_collector-worker"))
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "{}", serde_json::to_string(&config).unwrap()).unwrap();
    input.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    while fs::metadata(root.path().join("logs/device.log"))
        .map(|m| m.len())
        .unwrap_or(0)
        == 0
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    drop(input);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    let receipt: Receipt =
        serde_json::from_slice(&fs::read(root.path().join("receipt.json")).unwrap()).unwrap();
    assert!(receipt.child_reaped);
    assert_eq!(
        serde_json::to_value(receipt).unwrap()["reason"],
        "lease_closed"
    );
    let bytes = fs::read(root.path().join("logs/device.log")).unwrap();
    thread::sleep(Duration::from_millis(200));
    assert_eq!(
        bytes,
        fs::read(root.path().join("logs/device.log")).unwrap()
    );
}
#[test]
#[cfg(unix)]
fn abnormal_collector_outcomes_during_stop_remain_partial() {
    for (serial, exit_code) in [
        ("fixture-stop-exit", serde_json::json!(23)),
        ("fixture-stop-signal", Value::Null),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut owner = owner(root.path());
        start(&mut owner, serial, Limits::default());
        wait(&mut owner, |v| v["session"]["logs"] == 100);
        let stopped = serde_json::to_value(owner.stop().unwrap()).unwrap();
        let receipt = &stopped["session"]["receipt"];
        assert_eq!(receipt["exit_code"], exit_code, "{serial}");
        assert_eq!(receipt["incomplete_tail"], false, "{serial}");
        assert_eq!(receipt["stderr"], "", "{serial}");
        assert_eq!(receipt["forced"], false, "{serial}");
        assert_eq!(receipt["reason"], "collector_exit", "{serial}");
        assert_eq!(stopped["outcome"], "ready", "{serial}");
        let entries = zip_entries(&owner.export().unwrap());
        let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).unwrap();
        assert_eq!(manifest["package_state"], "partial", "{serial}");
        assert_eq!(
            manifest["completeness"]["device_logs"]["coverage_ended_before_session_end"], true,
            "{serial}"
        );
    }
}
#[test]
#[cfg(unix)]
fn intentional_sigterm_with_clean_logs_can_be_complete() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    start(&mut owner, "fixture-stop-clean", Limits::default());
    wait(&mut owner, |v| v["session"]["logs"] == 100);
    let stopped = serde_json::to_value(owner.stop().unwrap()).unwrap();
    assert_eq!(stopped["session"]["receipt"]["reason"], "lease_closed");
    assert_eq!(stopped["outcome"], "ready");
    let entries = zip_entries(&owner.export().unwrap());
    let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).unwrap();
    assert_eq!(manifest["package_state"], "complete");
    assert_eq!(
        manifest["completeness"]["device_logs"]["coverage_ended_before_session_end"],
        false
    );
}
#[test]
fn markerless_export_preserves_the_empty_raw_file() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    start(&mut owner, "fixture-stream", Limits::default());
    wait(&mut owner, |v| {
        v["session"]["bytes"].as_u64().unwrap_or(0) > 0
    });
    owner.stop().unwrap();
    let raw = fs::read(owner.directory().unwrap().join("timeline/markers.jsonl")).unwrap();
    assert!(raw.is_empty());
    let entries = zip_entries(&owner.export().unwrap());
    assert_eq!(entries.get("timeline/markers.jsonl"), Some(&raw));
    let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).unwrap();
    let artifact = manifest["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["path"] == "timeline/markers.jsonl")
        .unwrap();
    assert_eq!(artifact["bytes"], 0);
    assert_eq!(artifact["sha256"], format!("{:x}", Sha256::digest(&raw)));
    assert!(String::from_utf8(entries["SHA256SUMS"].clone())
        .unwrap()
        .contains(&format!(
            "{:x}  timeline/markers.jsonl\n",
            Sha256::digest(&raw)
        )));
}
#[test]
fn missing_marker_evidence_refuses_export() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    start(&mut owner, "fixture-stream", Limits::default());
    wait(&mut owner, |v| {
        v["session"]["bytes"].as_u64().unwrap_or(0) > 0
    });
    owner.marker("must not disappear".into()).unwrap();
    owner.stop().unwrap();
    let directory = owner.directory().unwrap();
    fs::remove_file(directory.join("timeline/markers.jsonl")).unwrap();
    assert!(owner.export().is_err());
    assert!(!owner
        .directory()
        .unwrap()
        .join("capture-package.zip")
        .exists());
}
#[test]
fn publication_failure_preserves_evidence_and_allows_retry() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    start(&mut owner, "fixture-stream", Limits::default());
    wait(&mut owner, |v| {
        v["session"]["bytes"].as_u64().unwrap_or(0) > 0
    });
    owner.stop().unwrap();
    let directory = owner.directory().unwrap().to_path_buf();
    let raw = fs::read(directory.join("logs/device.log")).unwrap();
    fs::create_dir(directory.join("capture-package.zip")).unwrap();
    assert!(owner.export().is_err());
    assert_eq!(raw, fs::read(directory.join("logs/device.log")).unwrap());
    fs::remove_dir(directory.join("capture-package.zip")).unwrap();
    owner.export().unwrap();
}
#[test]
fn evidence_links_and_nonregular_files_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    start(&mut owner, "fixture-stream", Limits::default());
    wait(&mut owner, |v| {
        v["session"]["bytes"].as_u64().unwrap_or(0) > 0
    });
    owner.stop().unwrap();
    let directory = owner.directory().unwrap().to_path_buf();
    let log = directory.join("logs/device.log");
    let bytes = fs::read(&log).unwrap();
    fs::remove_file(&log).unwrap();
    fs::create_dir(&log).unwrap();
    assert!(owner.export().is_err());
    fs::remove_dir(&log).unwrap();
    fs::write(&log, bytes).unwrap();
    #[cfg(unix)]
    {
        let elsewhere = root.path().join("external");
        fs::write(&elsewhere, b"outside").unwrap();
        fs::remove_file(&log).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &log).unwrap();
        assert!(owner.export().is_err());
    }
    #[cfg(windows)]
    {
        let saved = directory.join("logs-original");
        fs::rename(directory.join("logs"), &saved).unwrap();
        let status = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(directory.join("logs"))
            .arg(&saved)
            .status()
            .unwrap();
        assert!(
            status.success(),
            "Junction fixture must be available on Windows"
        );
        assert!(owner.export().is_err());
    }
}
#[test]
fn dropping_owner_closes_the_worker_lease() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    start(&mut owner, "fixture-stream", Limits::default());
    wait(&mut owner, |v| {
        v["session"]["bytes"].as_u64().unwrap_or(0) > 0
    });
    let directory = owner.directory().unwrap().to_path_buf();
    drop(owner);
    let receipt: Value =
        serde_json::from_slice(&fs::read(directory.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["child_reaped"], true);
}

#[test]
fn abrupt_owner_exit_ends_collection_without_a_destructor() {
    use std::io::{BufRead, BufReader};
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fixture-owner"))
        .arg(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut name = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut name)
        .unwrap();
    let directory = std::path::PathBuf::from(name.trim());
    let deadline = Instant::now() + Duration::from_secs(4);
    while fs::metadata(directory.join("logs/device.log"))
        .map(|m| m.len())
        .unwrap_or(0)
        == 0
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    #[cfg(windows)]
    let collector = {
        let pid = loop {
            let bytes = fs::read(directory.join("logs/device.log")).unwrap();
            if let Some(pid) = String::from_utf8_lossy(&bytes).lines().find_map(|line| {
                line.split_once("owned_pid=")
                    .and_then(|(_, pid)| pid.parse::<u32>().ok())
            }) {
                break pid;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        };
        let handle =
            unsafe { windows_sys::Win32::System::Threading::OpenProcess(0x00100000, 0, pid) };
        assert!(
            !handle.is_null(),
            "Retain a wait handle to the actual fake ADB before owner exit"
        );
        handle
    };
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    #[cfg(windows)]
    {
        let outcome =
            unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(collector, 3000) };
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(collector);
        }
        assert_eq!(
            outcome, 0,
            "The actual owned ADB process must exit after Job closure"
        );
    }
    assert!(
        !directory.join("capture-package.zip").exists(),
        "Abrupt exit must never publish evidence as a successful capture"
    );
    thread::sleep(Duration::from_millis(350));
    let bytes = fs::read(directory.join("logs/device.log")).unwrap();
    thread::sleep(Duration::from_millis(350));
    assert_eq!(
        bytes,
        fs::read(directory.join("logs/device.log")).unwrap(),
        "Logcat must stop when its owner exits abruptly"
    );
    #[cfg(unix)]
    {
        let receipt: Value =
            serde_json::from_slice(&fs::read(directory.join("receipt.json")).unwrap()).unwrap();
        assert_eq!(receipt["child_reaped"], true);
    }
    #[cfg(windows)]
    if let Ok(bytes) = fs::read(directory.join("receipt.json")) {
        let receipt: Receipt = serde_json::from_slice(&bytes).unwrap();
        assert!(
            receipt.child_reaped,
            "A worker that wins the Job-close race must have reaped its child"
        );
    }
}

#[test]
fn worker_failure_without_a_receipt_never_claims_verified_stop() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = CaptureOwner::open(
        CapturePaths {
            output_root: root.path().into(),
            runtime_root: Some(root.path().join("fixture-runtime")),
        },
        ToolSelection {
            adb: env!("CARGO_BIN_EXE_fake-adb").into(),
            worker: env!("CARGO_BIN_EXE_fixture-worker-crash").into(),
            guardian: PathBuf::new(),
            mitmdump: None,
            addon: PathBuf::new(),
        },
        true,
    )
    .unwrap();
    start(&mut owner, "fixture-stream", Limits::default());
    wait(&mut owner, |v| v["phase"] == "unverified_shutdown");
    assert!(owner.prepare_save().is_err());
    assert!(owner.export().is_err());
    assert_eq!(
        serde_json::to_value(owner.stop().unwrap()).unwrap()["outcome"],
        "unverified"
    );
    assert!(owner
        .start(StartRequest {
            serial: "fixture-stream".into(),
            whole_device_consent: true,
            selection: CaptureSelection::Logs,
            limits: Limits::default(),
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        })
        .is_err());
}

#[test]
fn recent_window_is_bounded_while_export_contains_every_record() {
    let root = tempfile::tempdir().unwrap();
    let tools = root.path().join("tools");
    fs::create_dir(&tools).unwrap();
    let adb = tools.join(format!("fake-adb{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_fake-adb"), &adb).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    fs::write(
        tools.join("fake-adb-flood-control"),
        listener.local_addr().unwrap().to_string(),
    )
    .unwrap();
    let mut owner = owner_with_adb(root.path(), adb);
    start(
        &mut owner,
        "fixture-flood-gated",
        Limits {
            records: 650,
            ..Limits::default()
        },
    );
    let ready = wait(&mut owner, |v| {
        v["session"]["logs"].as_u64().unwrap_or(0) >= 4
    });
    assert_eq!(ready["phase"], "recording");
    let (mut flood, _) = listener
        .accept()
        .expect("Flood fixture must connect before emitting its readiness records");
    let marker =
        b"{\"timestamp\":\"2026-10-15T00:00:01.500000Z\",\"text\":\"older fixture marker\"}\n";
    fs::write(
        owner.directory().unwrap().join("timeline/markers.jsonl"),
        marker,
    )
    .unwrap();
    flood.write_all(&[1]).unwrap();
    let state = wait(&mut owner, |v| v["phase"] == "stopped");
    assert_eq!(state["session"]["receipt"]["reason"], "record_limit");
    assert_eq!(state["session"]["receipt"]["records"], 650);
    assert_eq!(state["session"]["events"].as_array().unwrap().len(), 500);
    assert_eq!(state["session"]["markers"], 1);
    assert_eq!(
        state["session"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["source"] == "marker")
            .count(),
        1
    );
    let entries = zip_entries(&owner.export().unwrap());
    assert_eq!(entries["timeline/markers.jsonl"], marker);
    assert_eq!(
        entries["logs/device.log"]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count(),
        650
    );
    let count = entries["timeline/events.jsonl"]
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .count();
    assert_eq!(count as u64, state["session"]["logs"].as_u64().unwrap() + 1);
    assert_eq!(count, 650);
}

fn app_fixture(root: &Path) -> (CaptureOwner, PathBuf) {
    let tools = root.join("app-tools");
    fs::create_dir(&tools).unwrap();
    let adb = tools.join(format!("fake-adb{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_fake-adb"), &adb).unwrap();
    let state = tools.join("fake-android-state.json");
    fs::write(&state,serde_json::to_vec(&serde_json::json!({"user":0,"uid":10001,"install":"v1","shared":false,"uid_supported":true,"processes":true})).unwrap()).unwrap();
    (
        CaptureOwner::open(
            CapturePaths {
                output_root: root.join("output"),
                runtime_root: Some(root.join("private")),
            },
            ToolSelection {
                adb,
                worker: env!("CARGO_BIN_EXE_collector-worker").into(),
                guardian: PathBuf::new(),
                mitmdump: None,
                addon: PathBuf::new(),
            },
            true,
        )
        .unwrap(),
        state,
    )
}
fn app_request(serial: &str) -> StartRequest {
    StartRequest {
        serial: serial.into(),
        whole_device_consent: false,
        selection: CaptureSelection::Logs,
        limits: Limits::default(),
        log_scope: LogScope::App {
            application_id: "com.fixture.app".into(),
        },
        network_policy: NetworkPolicy::default(),
        context: CaptureContext {
            title: "App regression".into(),
            notes: "Private app scope".into(),
            steps: vec!["Open screen".into()],
        },
    }
}
#[test]
fn app_scope_discovers_active_user_and_persists_only_verified_owned_bytes() {
    for serial in ["fixture-app-uid", "fixture-app-pid"] {
        let root = tempfile::tempdir().unwrap();
        let (mut owner, _) = app_fixture(root.path());
        let adb = root
            .path()
            .join("app-tools")
            .join(format!("fake-adb{}", std::env::consts::EXE_SUFFIX));
        let apps = capture_poc_core::android::applications(&adb, serial).unwrap();
        assert_eq!(apps[0].application_id, "com.fixture.app");
        assert!(!apps[0].shared_uid);
        owner.start(app_request(serial)).unwrap();
        wait(&mut owner, |v| {
            v["session"]["logs"].as_u64().unwrap_or(0) > 2
        });
        let status = serde_json::to_value(owner.status().unwrap()).unwrap();
        assert!(status["session"]["events"].as_array().unwrap().is_empty());
        assert!(status["session"]["bytes"].as_u64().unwrap() > 0);
        owner.stop().unwrap();
        let directory = owner.directory().unwrap();
        let raw = fs::read_to_string(directory.join("logs/device.log")).unwrap();
        assert!(raw.contains("owned-"));
        assert!(raw.contains("owned-stack-"));
        assert!(!raw.contains("private-other"));
        assert!(!raw.contains("orphan private"));
        let entries = zip_entries(&owner.export().unwrap());
        let descriptor: Value = serde_json::from_slice(&entries["capture.json"]).unwrap();
        let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).unwrap();
        assert_eq!(descriptor["context"]["title"], "App regression");
        assert_eq!(descriptor["log_scope"]["application_id"], "com.fixture.app");
        assert_eq!(
            descriptor["log_attribution"]["strategy"],
            if serial.ends_with("uid") {
                "android_uid"
            } else {
                "android_process_map"
            }
        );
        assert_eq!(manifest["context"]["log_scope"], descriptor["log_scope"]);
        assert!(!entries.keys().any(|p| p.starts_with("runtime/")));
    }
}
#[test]
fn app_identity_changes_stop_before_unverified_raw_persistence() {
    for (field, value) in [
        ("install", serde_json::json!("v2")),
        ("user", serde_json::json!(10)),
        ("shared", serde_json::json!(true)),
        ("clock_offset", serde_json::json!(-60.0)),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut owner, state) = app_fixture(root.path());
        owner.start(app_request("fixture-app-uid")).unwrap();
        wait(&mut owner, |v| {
            v["session"]["logs"].as_u64().unwrap_or(0) > 2
        });
        let mut changed: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
        changed[field] = value;
        fs::write(&state, serde_json::to_vec(&changed).unwrap()).unwrap();
        let failed = wait(&mut owner, |v| v["phase"] == "failed");
        assert_eq!(failed["session"]["receipt"]["reason"], "identity_gap");
        let directory = owner.directory().unwrap();
        let size = fs::metadata(directory.join("logs/device.log"))
            .unwrap()
            .len();
        thread::sleep(Duration::from_millis(250));
        assert_eq!(
            fs::metadata(directory.join("logs/device.log"))
                .unwrap()
                .len(),
            size
        );
        assert!(failed["session"]["receipt"]["stderr"]
            .as_str()
            .unwrap()
            .contains("gap"));
        let entries = zip_entries(&owner.export().unwrap());
        let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).unwrap();
        assert_eq!(manifest["package_state"], "partial");
    }
}
#[test]
fn invalid_or_shared_app_never_broadens_scope() {
    let root = tempfile::tempdir().unwrap();
    let (mut owner, state) = app_fixture(root.path());
    let mut request = app_request("fixture-app-uid");
    request.log_scope = LogScope::App {
        application_id: "x'; reboot".into(),
    };
    assert!(owner.start(request).is_err());
    let mut shared: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    shared["shared"] = serde_json::json!(true);
    fs::write(state, serde_json::to_vec(&shared).unwrap()).unwrap();
    assert!(owner
        .start(app_request("fixture-app-uid"))
        .unwrap_err()
        .contains("shares"));
    assert!(owner.directory().is_none());
    assert!(!root.path().join("output").exists());
}
#[test]
fn saved_logs_reopen_and_export_only_with_private_verified_completion() {
    let root = tempfile::tempdir().unwrap();
    let mut first = owner(root.path());
    start(&mut first, "fixture-stream", Limits::default());
    wait(&mut first, |v| {
        v["session"]["logs"].as_u64().unwrap_or(0) > 2
    });
    let directory = first.directory().unwrap().to_owned();
    assert!(owner(root.path()).open_saved(directory.clone()).is_err());
    first.stop().unwrap();
    drop(first);
    let mut reopened = owner(root.path());
    assert!(matches!(
        reopened.open_saved(directory.clone()).unwrap(),
        Snapshot::Stopped(_)
    ));
    assert!(reopened.export().unwrap().is_file());
    fs::write(directory.join("capture.json"), b"{}").unwrap();
    assert!(reopened.export().is_err());
    drop(reopened);
    let root2 = tempfile::tempdir().unwrap();
    fs::create_dir(root2.path().join("timeline")).unwrap();
    fs::create_dir(root2.path().join("logs")).unwrap();
    fs::write(
        root2.path().join("session.json"),
        b"{\"phase\":\"stopped\"}",
    )
    .unwrap();
    fs::write(
        root2.path().join("receipt.json"),
        b"{\"child_reaped\":true}",
    )
    .unwrap();
    assert!(owner(root.path())
        .open_saved(root2.path().to_owned())
        .is_err());
}

#[test]
fn closing_app_source_cancels_an_inflight_identity_probe_before_worker_escalation() {
    let root = tempfile::tempdir().unwrap();
    let (mut owner, state) = app_fixture(root.path());
    owner.start(app_request("fixture-app-uid")).unwrap();
    wait(&mut owner, |v| {
        v["session"]["logs"].as_u64().unwrap_or(0) > 2
    });
    let mut slow: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    slow["probe_delay_ms"] = serde_json::json!(7000);
    fs::write(&state, serde_json::to_vec(&slow).unwrap()).unwrap();
    let probe_started = state.with_file_name("fake-android-probe-started");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !probe_started.exists() {
        assert!(
            Instant::now() < deadline,
            "Slow identity probe did not start"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!owner.directory().unwrap().join("receipt.json").exists());
    let started = Instant::now();
    let outcome = owner.stop().unwrap();
    let StopOutcome::Ready(stopped) = outcome else {
        panic!("Cancelled probe must still produce verified collector shutdown: {outcome:?}")
    };
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "Identity probe cancellation exceeded the worker shutdown deadline"
    );
    let receipt = stopped.receipt.unwrap();
    assert_eq!(receipt.reason, StopReason::LeaseClosed);
    assert!(receipt.child_reaped);
    assert_eq!(
        receipt.forced,
        cfg!(windows),
        "Windows normally terminates the owned ADB child; the worker must publish its own receipt"
    );
    let entries = zip_entries(&owner.export().unwrap());
    let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).unwrap();
    assert_eq!(
        manifest["collector_status"],
        serde_json::to_value(&receipt).unwrap()
    );
    if cfg!(windows) {
        assert_eq!(receipt.exit_code, Some(1));
        assert!(stopped
            .warnings
            .iter()
            .any(|warning| warning.contains("forced termination")));
        assert_eq!(manifest["package_state"], "partial");
        assert_eq!(manifest["completeness"]["partial"], true);
    }
}

#[test]
fn controlled_save_admits_verified_logs_cancels_preserves_and_retries_real_zip() {
    use capture_poc_core::save::{copy_evidence_archive, CancelOutcome};
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    assert!(owner.prepare_save().is_err());
    start(&mut owner, "fixture-stream", Limits::default());
    assert!(owner.prepare_save().is_err());
    wait(&mut owner, |v| {
        v["session"]["logs"].as_u64().unwrap_or(0) >= 4
    });
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    let first = owner.prepare_save().unwrap();
    let archive = owner.export_with_control(&first).unwrap();
    assert_eq!(first.progress().unwrap().stage, "complete");
    let prior = fs::read(&archive).unwrap();
    assert_eq!(owner.export_with_control(&first).unwrap(), archive);
    let directory = owner.directory().unwrap().to_path_buf();
    let raw = fs::read(directory.join("logs/device.log")).unwrap();
    let cancelled = owner.prepare_save().unwrap();
    assert_eq!(cancelled.cancel().unwrap(), CancelOutcome::Accepted);
    assert!(owner
        .export_with_control(&cancelled)
        .unwrap_err()
        .contains("cancelled"));
    assert_eq!(fs::read(&archive).unwrap(), prior);
    assert_eq!(fs::read(directory.join("logs/device.log")).unwrap(), raw);
    let retry = owner.prepare_save().unwrap();
    assert_ne!(retry.id(), cancelled.id());
    owner.export_with_control(&retry).unwrap();
    assert_eq!(fs::read(&archive).unwrap(), prior);
    let entries = zip_entries(&archive);
    assert!(entries.keys().all(|name| matches!(
        name.as_str(),
        "capture.json"
            | "context.json"
            | "logs/device.log"
            | "manifest.json"
            | "README.txt"
            | "SHA256SUMS"
            | "timeline/markers.jsonl"
            | "timeline/events.jsonl"
    )));
    for line in String::from_utf8(entries["SHA256SUMS"].clone())
        .unwrap()
        .lines()
    {
        let (hash, name) = line.split_once("  ").unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&entries[name])), hash);
    }
    let copied = root.path().join("copied.zip");
    fs::write(&copied, b"previous destination").unwrap();
    copy_evidence_archive(&archive, &copied).unwrap();
    assert_eq!(fs::read(&copied).unwrap(), prior);
    copy_evidence_archive(&archive, &copied).unwrap();
    assert_eq!(fs::read(&copied).unwrap(), prior);
    assert_eq!(fs::read(&archive).unwrap(), prior);
    start(&mut owner, "fixture-stream", Limits::default());
    assert!(owner.export_with_control(&first).is_err());
    wait(&mut owner, |v| {
        v["session"]["logs"].as_u64().unwrap_or(0) >= 1
    });
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
}

#[test]
fn immediate_stop_after_log_start_verifies_shutdown_and_can_save() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    for attempt in 0..8 {
        start(&mut owner, "fixture-stream", Limits::default());
        let stopped = owner.stop().unwrap();
        assert!(
            matches!(stopped, StopOutcome::Ready(_)),
            "immediate stop {attempt}: {stopped:?}"
        );
        let save = owner.prepare_save().unwrap();
        assert!(owner.export_with_control(&save).unwrap().is_file());
    }
}
