use capture_poc_core::{
    model::{Limits, Receipt, WorkerConfig},
    CaptureOwner,
};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn owner(root: &Path) -> CaptureOwner {
    CaptureOwner::new(
        env!("CARGO_BIN_EXE_fake-adb").into(),
        env!("CARGO_BIN_EXE_collector-worker").into(),
        root.into(),
        true,
    )
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
    owner.start(serial.into(), true, limits).unwrap();
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
        .start("fixture-stream".into(), false, Limits::default())
        .unwrap_err()
        .contains("consent"));
    assert!(owner
        .start("fixture-offline".into(), true, Limits::default())
        .is_err());
    assert!(owner
        .start("fixture-denied".into(), true, Limits::default())
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
    assert_eq!(stopped["phase"], "stopped");
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
fn startup_eof_starts_nothing_and_parent_eof_stops_the_collector() {
    let root = tempfile::tempdir().unwrap();
    let mut empty = Command::new(env!("CARGO_BIN_EXE_collector-worker"))
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    drop(empty.stdin.take());
    assert!(empty.wait().unwrap().success());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    let config = WorkerConfig {
        adb: env!("CARGO_BIN_EXE_fake-adb").into(),
        serial: "fixture-stream".into(),
        directory: root.path().into(),
        limits: Limits::default(),
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
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
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
    let mut owner = CaptureOwner::new(
        env!("CARGO_BIN_EXE_fake-adb").into(),
        env!("CARGO_BIN_EXE_fixture-worker-crash").into(),
        root.path().into(),
        true,
    );
    start(&mut owner, "fixture-stream", Limits::default());
    wait(&mut owner, |v| v["phase"] == "unverified_shutdown");
    assert!(owner.export().is_err());
    assert_eq!(
        serde_json::to_value(owner.stop().unwrap()).unwrap()["phase"],
        "unverified_shutdown"
    );
    assert!(owner
        .start("fixture-stream".into(), true, Limits::default())
        .is_err());
}

#[test]
fn recent_window_is_bounded_while_export_contains_every_record() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = owner(root.path());
    start(
        &mut owner,
        "fixture-flood",
        Limits {
            records: 650,
            ..Limits::default()
        },
    );
    let state = wait(&mut owner, |v| v["phase"] == "stopped");
    assert_eq!(state["session"]["events"].as_array().unwrap().len(), 500);
    let entries = zip_entries(&owner.export().unwrap());
    let count = entries["timeline/events.jsonl"]
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .count();
    assert_eq!(count as u64, state["session"]["logs"].as_u64().unwrap());
    assert!(count > 500);
}
