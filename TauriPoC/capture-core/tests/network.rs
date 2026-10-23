use capture_poc_core::{model::*, CaptureOwner};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    net::TcpStream,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    root: tempfile::TempDir,
    tools: ToolSelection,
    paths: CapturePaths,
    state: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self::from_binaries(None)
    }
    fn from_binaries(frozen: Option<&Path>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let tools_dir = root.path().join("tools");
        fs::create_dir(&tools_dir).unwrap();
        let copy = |source: &str, name: &str| {
            let path = tools_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            let source = frozen
                .map(|base| base.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
                .unwrap_or_else(|| PathBuf::from(source));
            fs::copy(source, &path).unwrap();
            path
        };
        let adb = copy(env!("CARGO_BIN_EXE_fake-adb"), "fake-adb");
        let engine = copy(env!("CARGO_BIN_EXE_fake-engine"), "fake-engine");
        let state = tools_dir.join("fake-adb-state.json");
        fs::write(&state,serde_json::to_vec(&json!({"settings":{},"reverse":{},"connected":true,"acknowledge_clear":true,"commands":[]})).unwrap()).unwrap();
        let tools = ToolSelection {
            adb,
            worker: frozen
                .map(|base| base.join(format!("collector-worker{}", std::env::consts::EXE_SUFFIX)))
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_collector-worker").into()),
            guardian: frozen
                .map(|base| base.join(format!("capture-guardian{}", std::env::consts::EXE_SUFFIX)))
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_capture-guardian").into()),
            mitmdump: Some(engine),
            addon: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../resources/network_addon.py")
                .canonicalize()
                .unwrap(),
        };
        let paths = CapturePaths {
            output_root: root.path().join("output"),
            runtime_root: Some(root.path().join("private-namespace")),
        };
        Self {
            root,
            tools,
            paths,
            state,
        }
    }
    fn owner(&self) -> CaptureOwner {
        CaptureOwner::open(self.paths.clone(), self.tools.clone(), true).unwrap()
    }
    fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.state).unwrap()).unwrap()
    }
    fn edit(&self, edit: impl FnOnce(&mut Value)) {
        let mut value = self.state();
        edit(&mut value);
        fs::write(&self.state, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn start(&self, owner: &mut CaptureOwner) -> Snapshot {
        owner
            .start(StartRequest {
                serial: "fixture-stream".into(),
                whole_device_consent: true,
                selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
                limits: Limits::default(),
                log_scope: LogScope::default(),
                network_policy: NetworkPolicy::default(),
                context: CaptureContext::default(),
            })
            .unwrap()
    }
}
fn listener(directory: &Path) -> bool {
    let route: Value =
        serde_json::from_slice(&fs::read(directory.join("runtime/routing.json")).unwrap()).unwrap();
    let port = route["endpoint"]
        .as_str()
        .unwrap()
        .trim_start_matches("tcp:")
        .parse::<u16>()
        .unwrap();
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}
fn wait_until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !check() {
        assert!(Instant::now() < deadline, "condition timed out");
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(windows)]
#[test]
fn windows_launch_probe_runs_inside_rust_test_job() {
    let probe = std::process::Command::new("python")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/probe-windows-launch.py"))
        .status()
        .expect("Windows launch probe must run from the Rust test process");
    let f = Fixture::new();
    let mut owner = f.owner();
    assert!(matches!(f.start(&mut owner), Snapshot::Recording(_)));
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    assert!(probe.success(), "owned-handle Windows launch probe failed");
}

#[test]
fn fixture_route_exact_values_marker_receipt_and_sealed_export() {
    let f = Fixture::new();
    let original = json!({"http_proxy":"null","global_http_proxy_host":"","global_http_proxy_exclusion_list":"a=b 'quoted'","global_proxy_pac_url":""});
    f.edit(|s| s["settings"] = original.clone());
    let mut owner = f.owner();
    assert!(matches!(f.start(&mut owner), Snapshot::Recording(_)));
    assert!(owner.export().is_err());
    let directory = owner.directory().unwrap().to_owned();
    assert!(listener(&directory));
    owner.marker("fixture network marker".into()).unwrap();
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    assert_eq!(f.state()["settings"], original);
    assert_eq!(f.state()["reverse"], json!({}));
    let archive = owner.export().unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    let mut manifest = String::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["capture_network"], true);
    assert_eq!(manifest["routing_restored"], true);
    assert_eq!(manifest["counts"]["markers"], 1);
    assert!(zip.by_name("network/capture.mitm").is_ok());
    assert!(zip.by_name("runtime/control.json").is_err());
    assert!(zip.by_name("runtime/routing.json").is_err());
    assert!(f.state()["commands"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| !v.to_string().contains("kill-server")));
}

#[test]
fn recent_preview_retains_network_after_log_flood_and_export_keeps_every_record() {
    let f = Fixture::new();
    let mut owner = f.owner();
    owner
        .start(StartRequest {
            serial: "fixture-flood".into(),
            whole_device_consent: true,
            selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
            limits: Limits {
                records: 600,
                ..Limits::default()
            },
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        })
        .unwrap();
    let directory = owner.directory().unwrap().to_owned();
    let records = ["request", "response"]
        .map(|kind| {
            format!(
                "{}\n",
                json!({"timestamp":"2026-10-15T00:00:01.500000Z","kind":kind,"summary":format!("fixture {kind}"),"detail":{},"clock_uncertainty_ms":1})
            )
        })
        .concat();
    fs::write(directory.join("network/events.jsonl"), &records).unwrap();
    fs::write(
        directory.join("runtime/coverage.json"),
        serde_json::to_vec(&CoverageView {
            decoded_http_events: 2,
            ..CoverageView::default()
        })
        .unwrap(),
    )
    .unwrap();
    let mut live = None;
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
        if !directory.join("receipt.json").is_file() {
            continue;
        }
        if let Snapshot::Recording(view) = owner.snapshot().unwrap() {
            if view.logs > 500 {
                live = Some(view);
                break;
            }
        }
    }
    let stopped = owner.stop().unwrap();
    let archive = owner.export().unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    let mut raw = String::new();
    zip.by_name("network/events.jsonl")
        .unwrap()
        .read_to_string(&mut raw)
        .unwrap();
    assert_eq!(raw, records);
    let mut normalized = String::new();
    zip.by_name("timeline/events.jsonl")
        .unwrap()
        .read_to_string(&mut normalized)
        .unwrap();
    let exported: Vec<Event> = normalized
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let live = live.expect("log flood must exceed the 500-event preview");
    assert_eq!(
        exported.iter().filter(|e| e.source == "logs").count(),
        live.logs
    );
    assert_eq!(exported.iter().filter(|e| e.source == "network").count(), 2);
    let StopOutcome::Ready(stopped) = stopped else {
        panic!("verified restoration required")
    };
    assert_eq!(stopped.logs, live.logs);
    for view in [live, stopped] {
        assert_eq!(view.markers, 0);
        let coverage = match &view.network {
            NetworkView::Recording(coverage) => coverage,
            NetworkView::Stopped(completion) => &completion.coverage,
            other => panic!("unexpected network state {other:?}"),
        };
        assert_eq!(coverage.decoded_http_events, 2);
        assert!(view.events.len() <= 500);
        assert_eq!(
            view.events.iter().filter(|e| e.source == "network").count(),
            2,
            "older network events must remain in a {}-event preview after {} logs",
            view.events.len(),
            view.logs
        );
    }
}

#[test]
fn disconnect_and_conflict_keep_listener_and_restore_on_retry() {
    let f = Fixture::new();
    let mut owner = f.owner();
    f.start(&mut owner);
    let directory = owner.directory().unwrap().to_owned();
    f.edit(|s| s["connected"] = json!(false));
    assert!(matches!(
        owner.stop().unwrap(),
        StopOutcome::RecoveryRequired(_)
    ));
    assert!(listener(&directory));
    assert!(owner.export().is_err());
    f.edit(|s| {
        s["connected"] = json!(true);
        s["settings"]["global_proxy_pac_url"] = json!("external");
    });
    assert!(matches!(
        owner.recover().unwrap(),
        StopOutcome::RecoveryRequired(_)
    ));
    assert_eq!(f.state()["settings"]["global_proxy_pac_url"], "external");
    assert!(listener(&directory));
    f.edit(|s| s["settings"]["global_proxy_pac_url"] = json!(""));
    assert!(matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));
    assert_eq!(f.state()["settings"], json!({}));
    assert!(owner.export().is_ok());
}
#[test]
fn gui_lease_eof_recovers_and_other_output_root_attaches() {
    let f = Fixture::new();
    let mut owner = f.owner();
    f.start(&mut owner);
    let directory = owner.directory().unwrap().to_owned();
    f.edit(|s| s["connected"] = json!(false));
    drop(owner);
    wait_until(|| {
        let header: Value =
            serde_json::from_slice(&fs::read(directory.join("session.json")).unwrap()).unwrap();
        header["error"].is_string()
    });
    assert!(listener(&directory));
    let mut paths = f.paths.clone();
    paths.output_root = f.root.path().join("another-root");
    let mut resumed = CaptureOwner::open(paths, f.tools.clone(), true).unwrap();
    assert_eq!(resumed.directory(), Some(directory.as_path()));
    assert!(matches!(
        resumed.snapshot().unwrap(),
        Snapshot::RecoveryRequired(_)
    ));
    f.edit(|s| s["connected"] = json!(true));
    assert!(matches!(resumed.recover().unwrap(), StopOutcome::Ready(_)));
}
#[test]
fn failed_takeover_does_not_destroy_live_control_or_bootstrap() {
    let f = Fixture::new();
    let mut owner = f.owner();
    f.start(&mut owner);
    let directory = owner.directory().unwrap().to_owned();
    let original = fs::read(directory.join("runtime/control.json")).unwrap();
    let bootstrap = fs::read(directory.join("runtime/bootstrap.json")).unwrap();
    let mut config: Value = serde_json::from_slice(&bootstrap).unwrap();
    config["recovery"] = json!(true);
    let input = f.root.path().join("contender.json");
    fs::write(&input, serde_json::to_vec(&config).unwrap()).unwrap();
    let result = std::process::Command::new(&f.tools.guardian)
        .arg(input)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        fs::read(directory.join("runtime/control.json")).unwrap(),
        original
    );
    assert_eq!(
        fs::read(directory.join("runtime/bootstrap.json")).unwrap(),
        bootstrap
    );
    assert!(matches!(owner.snapshot().unwrap(), Snapshot::Recording(_)));
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
}

fn framed(stream: &mut TcpStream, token: &str, action: Value) -> Value {
    use std::io::Write;
    let data = serde_json::to_vec(&json!({"token":token,"command":action})).unwrap();
    stream
        .write_all(&(data.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&data).unwrap();
    let mut len = [0; 4];
    stream.read_exact(&mut len).unwrap();
    let mut reply = vec![0; u32::from_be_bytes(len) as usize];
    stream.read_exact(&mut reply).unwrap();
    serde_json::from_slice(&reply).unwrap()
}
#[test]
fn hard_guardian_death_recovers_original_files_without_saved_pid_authority() {
    let f = Fixture::new();
    let directory = f.root.path().join("orphan-session");
    fs::create_dir_all(directory.join("runtime")).unwrap();
    let namespace = f.paths.runtime_root.as_ref().unwrap();
    let input = directory.join("runtime/test-start.json");
    let config = json!({"paths":f.paths,"tools":f.tools,"fixture":true,"request":{"serial":"fixture-stream","whole_device_consent":true,"selection":{"mode":"logs_and_network","consent":{"whole_device":true}},"limits":Limits::default()},"directory":directory,"recovery":false,"ca":namespace.join("ca"),"lock":namespace.join("owner.lock")});
    fs::write(&input, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut guardian = std::process::Command::new(&f.tools.guardian)
        .arg(&input)
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    wait_until(|| input.with_extension("ready").exists());
    let descriptor: Value =
        serde_json::from_slice(&fs::read(directory.join("runtime/control.json")).unwrap()).unwrap();
    let mut gui =
        TcpStream::connect(("127.0.0.1", descriptor["port"].as_u64().unwrap() as u16)).unwrap();
    gui.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let reply = framed(
        &mut gui,
        descriptor["token"].as_str().unwrap(),
        json!("Attach"),
    );
    assert_eq!(reply["Snapshot"]["phase"], "recording");
    wait_until(|| directory.join("logs/device.log").exists());
    guardian.kill().unwrap();
    guardian.wait().unwrap();
    drop(gui);
    let mut paths = f.paths.clone();
    paths.output_root = f.root.path().join("different-root");
    let mut resumed = CaptureOwner::open(paths, f.tools.clone(), true).unwrap();
    assert!(matches!(
        resumed.snapshot().unwrap(),
        Snapshot::RecoveryRequired(_)
    ));
    let stop = resumed.recover().unwrap();
    let StopOutcome::Ready(view) = stop else {
        panic!("recovery failed {stop:?}");
    };
    assert!(matches!(view.network, NetworkView::Interrupted(_)));
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
    let archive = resumed.export().unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let manifest: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(manifest["package_state"], "partial");
    assert_eq!(manifest["network_completion"]["interrupted"], true);
    assert!(!text.contains("public_certificate"));
    assert!(!text.contains("synthetic_check_url"));
    assert!(!text.contains(namespace.to_str().unwrap()));
}
#[test]
fn cache_clear_requires_full_ack_and_retry_uses_empty_transition() {
    let f = Fixture::new();
    let original = json!({"http_proxy":"","global_http_proxy_exclusion_list":"literal-null"});
    f.edit(|s| s["settings"] = original.clone());
    let mut owner = f.owner();
    f.start(&mut owner);
    let directory = owner.directory().unwrap().to_owned();
    f.edit(|s| s["acknowledge_clear"] = json!(false));
    assert!(matches!(
        owner.stop().unwrap(),
        StopOutcome::RecoveryRequired(_)
    ));
    assert!(listener(&directory));
    let count = f.state()["commands"].as_array().unwrap().len();
    f.edit(|s| s["acknowledge_clear"] = json!(true));
    assert!(matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));
    let state = f.state();
    let cmds = state["commands"].as_array().unwrap();
    let writes: Vec<_> = cmds[count..]
        .iter()
        .filter_map(|v| v[1].as_str())
        .filter(|s| s.starts_with("settings put global 'http_proxy'"))
        .collect();
    assert!(writes.len() >= 2);
    assert!(writes[0].ends_with("''"));
    assert!(writes[1].ends_with("':0'"));
    assert_eq!(state["settings"], original);
}
#[test]
fn every_claim_command_failure_preserves_exact_originals_on_retry() {
    for fail in 1..=24 {
        let f = Fixture::new();
        let original = json!({"http_proxy":"", "global_http_proxy_port":"0", "global_http_proxy_exclusion_list":"original=quoted 'value'"});
        f.edit(|s| {
            s["settings"] = original.clone();
            s["fail_command"] = json!(fail);
        });
        let mut owner = f.owner();
        let _ = owner.start(StartRequest {
            serial: "fixture-stream".into(),
            whole_device_consent: true,
            selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
            limits: Limits::default(),
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        });
        f.edit(|s| s["fail_command"] = Value::Null);
        if owner.directory().is_some() {
            assert!(
                matches!(owner.recover().unwrap(), StopOutcome::Ready(_)),
                "failure at command {fail}"
            );
        }
        assert_eq!(f.state()["settings"], original, "failure at command {fail}");
        assert_eq!(f.state()["reverse"], json!({}));
    }
}

#[test]
fn actual_gui_process_death_leaves_guardian_independent_and_saved_adb_absolute() {
    use std::io::BufRead;
    let f = Fixture::new();
    let mut tools = f.tools.clone();
    tools.adb = "fake-adb".into();
    let input = f.root.path().join("gui-config.json");
    fs::write(
        &input,
        serde_json::to_vec(&json!({"paths":f.paths,"tools":tools})).unwrap(),
    )
    .unwrap();
    let mut gui = std::process::Command::new(env!("CARGO_BIN_EXE_fixture-network-owner"))
        .arg(input)
        .env("PATH", f.tools.adb.parent().unwrap())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(gui.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let view: Value = serde_json::from_str(&line)
        .expect("GUI fixture must start a recording guardian even in Windows runner Job context");
    assert_eq!(view["phase"], "recording");
    let directory = PathBuf::from(view["session"]["directory"].as_str().unwrap());
    let route: Value =
        serde_json::from_slice(&fs::read(directory.join("runtime/routing.json")).unwrap()).unwrap();
    assert!(Path::new(route["adb"].as_str().unwrap()).is_absolute());
    assert_eq!(
        Path::new(route["adb"].as_str().unwrap()),
        fs::canonicalize(&f.tools.adb).unwrap()
    );
    f.edit(|s| s["connected"] = json!(false));
    gui.kill().unwrap();
    gui.wait().unwrap();
    wait_until(|| {
        let value: Value =
            serde_json::from_slice(&fs::read(directory.join("session.json")).unwrap()).unwrap();
        value["error"].is_string()
    });
    assert!(listener(&directory));
    let mut owner = f.owner();
    assert!(matches!(
        owner.snapshot().unwrap(),
        Snapshot::RecoveryRequired(_)
    ));
    f.edit(|s| s["connected"] = json!(true));
    assert!(matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));
    assert_eq!(f.state()["settings"], json!({}));
}
#[test]
fn engine_death_during_disconnect_reestablishes_forwarding_until_retry() {
    let f = Fixture::new();
    let mut owner = f.owner();
    f.start(&mut owner);
    let directory = owner.directory().unwrap().to_owned();
    f.edit(|s| s["connected"] = json!(false));
    let control = f
        .tools
        .mitmdump
        .as_ref()
        .unwrap()
        .with_file_name("fake-engine-control.json");
    fs::write(&control, b"{\"exit\":true}").unwrap();
    wait_until(|| {
        directory.join("runtime/engine-config.json").exists()
            && serde_json::from_slice::<Value>(
                &fs::read(directory.join("runtime/engine-config.json")).unwrap(),
            )
            .unwrap()["forwarding"]
                == true
    });
    fs::write(&control, b"{}").unwrap();
    wait_until(|| listener(&directory));
    assert!(matches!(
        owner.snapshot().unwrap(),
        Snapshot::RecoveryRequired(_)
    ));
    assert!(owner.export().is_err());
    f.edit(|s| s["connected"] = json!(true));
    assert!(matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));
    assert_eq!(f.state()["settings"], json!({}));
    let archive = owner.export().unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let manifest: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(manifest["package_state"], "partial");
}
#[test]
fn external_reverse_is_preserved_and_retry_never_kills_shared_server() {
    let f = Fixture::new();
    let mut owner = f.owner();
    f.start(&mut owner);
    let directory = owner.directory().unwrap().to_owned();
    let route: Value =
        serde_json::from_slice(&fs::read(directory.join("runtime/routing.json")).unwrap()).unwrap();
    let endpoint = route["endpoint"].as_str().unwrap();
    f.edit(|s| s["reverse"][endpoint] = json!("tcp:external"));
    assert!(matches!(
        owner.stop().unwrap(),
        StopOutcome::RecoveryRequired(_)
    ));
    assert!(listener(&directory));
    assert_eq!(f.state()["reverse"][endpoint], "tcp:external");
    f.edit(|s| s["reverse"][endpoint] = json!(endpoint));
    assert!(matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));
    assert_eq!(f.state()["reverse"], json!({}));
    assert!(f.state()["commands"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| !c.to_string().contains("kill-server")));
}

#[test]
fn second_source_lock_blocks_two_retries_until_both_original_writers_quiesce() {
    let f = Fixture::new();
    let directory = f.root.path().join("source-blocked-session");
    fs::create_dir_all(directory.join("runtime")).unwrap();
    let namespace = f.paths.runtime_root.as_ref().unwrap();
    let input = directory.join("runtime/test-start.json");
    let config = json!({"paths":f.paths,"tools":f.tools,"fixture":true,"request":{"serial":"fixture-stream","whole_device_consent":true,"selection":{"mode":"logs_and_network","consent":{"whole_device":true}},"limits":Limits::default()},"directory":directory,"recovery":false,"ca":namespace.join("ca"),"lock":namespace.join("owner.lock")});
    fs::write(&input, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut guardian = std::process::Command::new(&f.tools.guardian)
        .arg(&input)
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    wait_until(|| input.with_extension("ready").exists());
    let descriptor: Value =
        serde_json::from_slice(&fs::read(directory.join("runtime/control.json")).unwrap()).unwrap();
    let mut gui =
        TcpStream::connect(("127.0.0.1", descriptor["port"].as_u64().unwrap() as u16)).unwrap();
    gui.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    assert_eq!(
        framed(
            &mut gui,
            descriptor["token"].as_str().unwrap(),
            json!("Attach")
        )["Snapshot"]["phase"],
        "recording"
    );
    guardian.kill().unwrap();
    guardian.wait().unwrap();
    drop(gui);
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("runtime/network.writer"))
        .unwrap();
    wait_until(|| held.try_lock().is_ok());
    let mut owner = f.owner();
    assert!(!matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));
    assert!(owner.export().is_err());
    assert!(!matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));
    assert!(owner.export().is_err());
    drop(held);
    assert!(matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));
    assert!(owner.export().is_ok());
}

fn crash_session(
    f: &Fixture,
    at: Option<usize>,
    split_frame: bool,
) -> (std::process::Child, TcpStream, PathBuf) {
    use std::io::Write;
    let directory = f.root.path().join("crash-boundary-session");
    fs::create_dir_all(directory.join("runtime")).unwrap();
    if let Some(at) = at {
        fs::write(
            directory.join("runtime/fixture-crash.json"),
            serde_json::to_vec(&json!({"at":at})).unwrap(),
        )
        .unwrap();
    }
    let namespace = f.paths.runtime_root.as_ref().unwrap();
    let input = directory.join("runtime/test-start.json");
    let config = json!({"paths":f.paths,"tools":f.tools,"fixture":true,"request":{"serial":"fixture-stream","whole_device_consent":true,"selection":{"mode":"logs_and_network","consent":{"whole_device":true}},"limits":Limits::default()},"directory":directory,"recovery":false,"ca":namespace.join("ca"),"lock":namespace.join("owner.lock")});
    fs::write(&input, serde_json::to_vec(&config).unwrap()).unwrap();
    let child = std::process::Command::new(&f.tools.guardian)
        .arg(&input)
        .stdin(std::process::Stdio::null())
        .stderr(fs::File::create(directory.join("runtime/guardian.stderr.private")).unwrap())
        .spawn()
        .unwrap();
    wait_until(|| input.with_extension("ready").exists());
    let descriptor: Value =
        serde_json::from_slice(&fs::read(directory.join("runtime/control.json")).unwrap()).unwrap();
    let mut gui =
        TcpStream::connect(("127.0.0.1", descriptor["port"].as_u64().unwrap() as u16)).unwrap();
    gui.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let bytes =
        serde_json::to_vec(&json!({"token":descriptor["token"],"command":"Attach"})).unwrap();
    let length = (bytes.len() as u32).to_be_bytes();
    if split_frame {
        gui.write_all(&length[..1]).unwrap();
        thread::sleep(Duration::from_millis(100));
        gui.write_all(&length[1..]).unwrap();
        thread::sleep(Duration::from_millis(100));
        for part in bytes.chunks(8) {
            gui.write_all(part).unwrap();
            thread::sleep(Duration::from_millis(1));
        }
    } else {
        gui.write_all(&length).unwrap();
        gui.write_all(&bytes).unwrap();
    }
    let mut len = [0; 4];
    match gui.read_exact(&mut len) {
        Ok(()) => {
            let mut reply = vec![0; u32::from_be_bytes(len) as usize];
            gui.read_exact(&mut reply).unwrap();
            fs::write(directory.join("runtime/fixture-attach-reply.json"), reply).unwrap();
        }
        Err(error) => {
            fs::write(
                directory.join("runtime/fixture-attach-error.json"),
                serde_json::to_vec(
                    &json!({"kind":format!("{:?}", error.kind()),"message":error.to_string()}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
    (child, gui, directory)
}
fn request_stop(directory: &Path) {
    use std::io::Write;
    let descriptor: Value =
        serde_json::from_slice(&fs::read(directory.join("runtime/control.json")).unwrap()).unwrap();
    if let Ok(mut stream) =
        TcpStream::connect(("127.0.0.1", descriptor["port"].as_u64().unwrap() as u16))
    {
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let bytes =
            serde_json::to_vec(&json!({"token":descriptor["token"],"command":"Stop"})).unwrap();
        let _ = stream.write_all(&(bytes.len() as u32).to_be_bytes());
        let _ = stream.write_all(&bytes);
        let mut len = [0; 4];
        if stream.read_exact(&mut len).is_ok() {
            let mut reply = vec![0; u32::from_be_bytes(len) as usize];
            let _ = stream.read_exact(&mut reply);
        }
    }
}

#[cfg(unix)]
#[test]
fn completed_network_handoff_waits_for_previous_guardian_release() {
    let f = Fixture::new();
    let (mut guardian, gui, directory) = crash_session(&f, None, false);
    let mut owner = f.owner();
    drop(gui);
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    let archive = owner.export().unwrap();
    let original_archive = fs::read(&archive).unwrap();
    assert!(guardian.try_wait().unwrap().is_none());
    assert_eq!(
        unsafe { libc::kill(guardian.id() as i32, libc::SIGSTOP) },
        0
    );
    let control = directory.join("runtime/control.json");
    let unavailable = directory.join("runtime/control.paused.json");
    fs::rename(&control, &unavailable).unwrap();
    let resume = thread::spawn(move || {
        thread::sleep(Duration::from_millis(250));
        fs::rename(unavailable, control).unwrap();
        assert_eq!(
            unsafe { libc::kill(guardian.id() as i32, libc::SIGCONT) },
            0
        );
        guardian.wait().unwrap()
    });
    let started = owner.start(StartRequest {
        serial: "fixture-stream".into(),
        whole_device_consent: true,
        selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
        limits: Limits::default(),
        log_scope: LogScope::default(),
        network_policy: NetworkPolicy::default(),
        context: CaptureContext::default(),
    });
    assert!(resume.join().unwrap().success());
    assert!(matches!(started, Ok(Snapshot::Recording(_))), "{started:?}");
    assert_ne!(owner.directory(), Some(directory.as_path()));
    assert_eq!(fs::read(archive).unwrap(), original_archive);
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    assert!(owner.export().is_ok());
}

#[test]
fn completed_network_handoff_refuses_other_client_and_preserves_export() {
    let f = Fixture::new();
    let (mut guardian, gui, directory) = crash_session(&f, None, false);
    let mut owner = f.owner();
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    let archive = owner.export().unwrap();
    let commands = f.state()["commands"].clone();
    let locator = fs::read(f.paths.runtime_root.as_ref().unwrap().join("owner.lock")).unwrap();
    let refusal = Instant::now();
    let started = owner.start(StartRequest {
        serial: "fixture-stream".into(),
        whole_device_consent: true,
        selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
        limits: Limits::default(),
        log_scope: LogScope::default(),
        network_policy: NetworkPolicy::default(),
        context: CaptureContext::default(),
    });
    assert!(refusal.elapsed() < Duration::from_secs(8));
    assert_eq!(
        started.unwrap_err(),
        "Another capture owns this connection or source"
    );
    assert!(guardian.try_wait().unwrap().is_none());
    assert_eq!(owner.directory(), Some(directory.as_path()));
    assert!(matches!(owner.snapshot().unwrap(), Snapshot::Stopped(_)));
    assert_eq!(owner.export().unwrap(), archive);
    assert_eq!(f.state()["commands"], commands);
    assert_eq!(
        fs::read(f.paths.runtime_root.as_ref().unwrap().join("owner.lock")).unwrap(),
        locator
    );
    assert!(!f.paths.output_root.exists());
    drop(gui);
    assert!(matches!(f.start(&mut owner), Snapshot::Recording(_)));
    assert!(guardian.wait().unwrap().success());
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    assert!(owner.export().is_ok());
}

#[test]
fn completed_network_handoff_after_directory_moves_preserves_evidence() {
    let f = Fixture::new();
    let (mut guardian, gui, directory) = crash_session(&f, None, false);
    let mut owner = f.owner();
    drop(gui);
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    let archive = owner.export().unwrap();
    let original_archive = fs::read(&archive).unwrap();
    let moved = f.root.path().join("moved-completed-session");
    fs::rename(&directory, &moved).unwrap();

    let snapshot_error = owner.snapshot().unwrap_err();
    let export_error = owner.export().unwrap_err();
    let started = owner.start(StartRequest {
        serial: "fixture-stream".into(),
        whole_device_consent: true,
        selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
        limits: Limits::default(),
        log_scope: LogScope::default(),
        network_policy: NetworkPolicy::default(),
        context: CaptureContext::default(),
    });
    if started.is_err() {
        fs::rename(&moved, &directory).unwrap();
    }
    assert!(matches!(started, Ok(Snapshot::Recording(_))), "{started:?}");
    assert!(guardian.wait().unwrap().success());
    assert!(!directory.exists());
    assert_ne!(owner.directory(), Some(directory.as_path()));
    for error in [snapshot_error, export_error] {
        assert!(error.contains(&directory.display().to_string()), "{error}");
        assert!(error.contains("missing"), "{error}");
    }
    assert_eq!(
        fs::read(moved.join(archive.strip_prefix(&directory).unwrap())).unwrap(),
        original_archive
    );
    owner.marker("capture after moved evidence".into()).unwrap();
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    assert!(owner.export().is_ok());
    assert!(!directory.exists());
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
}

#[test]
fn missing_network_state_cannot_release_active_or_recovery_required_session() {
    for recovery_required in [false, true] {
        let f = Fixture::new();
        let mut owner = f.owner();
        f.start(&mut owner);
        if recovery_required {
            f.edit(|s| s["connected"] = json!(false));
            assert!(matches!(
                owner.stop().unwrap(),
                StopOutcome::RecoveryRequired(_)
            ));
        }
        let directory = owner.directory().unwrap().to_owned();
        let commands = f.state()["commands"].clone();
        let lock = f.paths.runtime_root.as_ref().unwrap().join("owner.lock");
        let locator = fs::read(&lock).unwrap();
        let moved = f.root.path().join("moved-unfinished-session");
        fs::rename(&directory, &moved).unwrap();
        let started = owner.start(StartRequest {
            serial: "fixture-stream".into(),
            whole_device_consent: true,
            selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
            limits: Limits::default(),
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        });
        let unchanged_commands = f.state()["commands"] == commands;
        let unchanged_locator = fs::read(&lock).unwrap() == locator;
        let original_absent = !directory.exists();
        let retained_directory = owner.directory() == Some(directory.as_path());
        fs::rename(&moved, &directory).unwrap();
        f.edit(|s| s["connected"] = json!(true));
        assert!(matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));

        let error = started.unwrap_err();
        assert!(error.contains(&directory.display().to_string()), "{error}");
        assert!(error.contains("missing"), "{error}");
        assert!(unchanged_commands);
        assert!(unchanged_locator);
        assert!(original_absent);
        assert!(retained_directory);
    }
}

#[test]
fn offline_completion_cannot_authorize_handoff_after_state_disappears() {
    let f = Fixture::new();
    let (mut guardian, gui, directory) = crash_session(&f, None, false);
    request_stop(&directory);
    drop(gui);
    assert!(guardian.wait().unwrap().success());
    let mut owner = f.owner();
    assert!(matches!(owner.snapshot().unwrap(), Snapshot::Stopped(_)));
    let commands = f.state()["commands"].clone();
    let moved = f.root.path().join("moved-offline-session");
    fs::rename(&directory, &moved).unwrap();
    let started = owner.start(StartRequest {
        serial: "fixture-stream".into(),
        whole_device_consent: true,
        selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
        limits: Limits::default(),
        log_scope: LogScope::default(),
        network_policy: NetworkPolicy::default(),
        context: CaptureContext::default(),
    });
    let original_absent = !directory.exists();
    fs::rename(&moved, &directory).unwrap();
    assert!(started.is_err(), "{started:?}");
    assert_eq!(owner.directory(), Some(directory.as_path()));
    assert_eq!(f.state()["commands"], commands);
    assert!(original_absent);
}

#[test]
fn missing_completed_state_still_requires_actual_owner_lease_handoff() {
    let f = Fixture::new();
    let (mut guardian, gui, directory) = crash_session(&f, None, false);
    let mut owner = f.owner();
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    let archive = owner.export().unwrap();
    let original_archive = fs::read(&archive).unwrap();
    let commands = f.state()["commands"].clone();
    let moved = f.root.path().join("moved-shared-session");
    fs::rename(&directory, &moved).unwrap();
    let started = owner.start(StartRequest {
        serial: "fixture-stream".into(),
        whole_device_consent: true,
        selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
        limits: Limits::default(),
        log_scope: LogScope::default(),
        network_policy: NetworkPolicy::default(),
        context: CaptureContext::default(),
    });
    if started.as_ref().err().map(String::as_str)
        != Some("Another capture owns this connection or source")
    {
        fs::rename(&moved, &directory).unwrap();
    }
    assert_eq!(
        started.unwrap_err(),
        "Another capture owns this connection or source"
    );
    assert!(guardian.try_wait().unwrap().is_none());
    assert_eq!(owner.directory(), Some(directory.as_path()));
    assert_eq!(f.state()["commands"], commands);
    assert!(!directory.exists());
    drop(gui);
    assert!(matches!(f.start(&mut owner), Snapshot::Recording(_)));
    assert!(guardian.wait().unwrap().success());
    assert_eq!(
        fs::read(moved.join(archive.strip_prefix(&directory).unwrap())).unwrap(),
        original_archive
    );
    assert!(!directory.exists());
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
}

#[test]
fn missing_completed_state_can_retry_a_launch_failure_before_pending_routing() {
    let frozen = freeze_binaries();
    let f = Fixture::from_binaries(Some(frozen.path()));
    let (mut guardian, gui, directory) = crash_session(&f, None, false);
    let mut owner = f.owner();
    drop(gui);
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    let moved = f.root.path().join("moved-before-launch-failure");
    fs::rename(&directory, &moved).unwrap();
    let unavailable = frozen.path().join("unavailable-guardian");
    fs::rename(&f.tools.guardian, &unavailable).unwrap();
    let started = owner.start(StartRequest {
        serial: "fixture-stream".into(),
        whole_device_consent: true,
        selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
        limits: Limits::default(),
        log_scope: LogScope::default(),
        network_policy: NetworkPolicy::default(),
        context: CaptureContext::default(),
    });
    fs::rename(&unavailable, &f.tools.guardian).unwrap();
    assert!(started
        .unwrap_err()
        .contains("Cannot launch independent guardian"));
    assert!(guardian.wait().unwrap().success());
    assert_eq!(owner.directory(), Some(directory.as_path()));
    assert!(!directory.exists());
    assert!(matches!(f.start(&mut owner), Snapshot::Recording(_)));
    assert!(!directory.exists());
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
}

#[test]
fn completed_network_handoff_repeats_network_then_log_capture() {
    let f = Fixture::new();
    let mut owner = f.owner();
    let mut directories = std::collections::HashSet::new();
    for _ in 0..3 {
        assert!(matches!(f.start(&mut owner), Snapshot::Recording(_)));
        assert!(directories.insert(owner.directory().unwrap().to_owned()));
        assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
        assert!(owner.export().is_ok());
        let directory = owner.directory().unwrap().to_owned();
        drop(owner);
        wait_until(|| {
            fs::read_to_string(directory.join("runtime/guardian-events.jsonl"))
                .unwrap_or_default()
                .contains("exit_quiescent_no_clients")
        });
        owner = f.owner();
        assert!(owner.export().is_ok());
    }
    assert!(matches!(
        owner
            .start(StartRequest {
                serial: "fixture-stream".into(),
                whole_device_consent: true,
                selection: CaptureSelection::Logs,
                limits: Limits::default(),
                log_scope: LogScope::default(),
                network_policy: NetworkPolicy::default(),
                context: CaptureContext::default(),
            })
            .unwrap(),
        Snapshot::Recording(_)
    ));
    assert!(directories.insert(owner.directory().unwrap().to_owned()));
    owner.marker("log-only after network".into()).unwrap();
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    let archive = owner.export().unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    assert!(zip.by_name("logs/device.log").is_ok());
    assert!(zip.by_name("network/capture.mitm").is_err());
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
}

#[test]
fn crash_before_and_after_every_route_journal_command_and_verification_recovers() {
    let frozen = freeze_binaries();
    let baseline = Fixture::from_binaries(Some(frozen.path()));
    let (mut child, gui, directory) = crash_session(&baseline, None, false);
    request_stop(&directory);
    let trace = fs::read_to_string(directory.join("runtime/route-boundaries.jsonl")).unwrap();
    let boundaries: Vec<Value> = trace
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for label in [
        "before_journal",
        "after_journal",
        "before_command",
        "after_command",
        "before_verify",
        "after_verify",
    ] {
        assert!(boundaries.iter().any(|b| b["boundary"] == label));
    }
    drop(gui);
    child.wait().unwrap();
    for boundary in &boundaries {
        let at = boundary["index"].as_u64().unwrap() as usize;
        let f = Fixture::from_binaries(Some(frozen.path()));
        let (mut child, gui, directory) = crash_session(&f, Some(at), false);
        if child.try_wait().unwrap().is_none() {
            request_stop(&directory);
        }
        drop(gui);
        let status = child.wait().unwrap();
        if status.code() != Some(91) {
            let events = fs::read_to_string(directory.join("runtime/guardian-events.jsonl"))
                .unwrap_or_default();
            let reply = fs::read_to_string(directory.join("runtime/fixture-attach-reply.json"))
                .unwrap_or_default();
            let error = fs::read_to_string(directory.join("runtime/fixture-attach-error.json"))
                .unwrap_or_default();
            let state = f.state();
            let preserved = f.root.keep();
            let binaries = frozen.keep();
            panic!("boundary {boundary}; status={status}; preserved={} binaries={} reply={reply} error={error} events={events} state={state}", preserved.display(), binaries.display());
        }
        fs::remove_file(directory.join("runtime/fixture-crash.json")).unwrap();
        let mut owner = f.owner();
        let outcome = match owner.recover() {
            Ok(outcome) => outcome,
            Err(error) => {
                let trace = fs::read_to_string(directory.join("runtime/guardian-events.jsonl"))
                    .unwrap_or_default();
                drop(owner);
                let preserved = f.root.keep();
                let binaries = frozen.keep();
                panic!("boundary {boundary}; recovery {error}; preserved={} binaries={} events={trace}",preserved.display(),binaries.display());
            }
        };
        assert!(
            matches!(outcome, StopOutcome::Ready(_)),
            "boundary {boundary}: {outcome:?}"
        );
        assert_eq!(f.state()["settings"], json!({}), "boundary {boundary}");
        assert_eq!(f.state()["reverse"], json!({}), "boundary {boundary}");
    }
    eprintln!(
        "Recovered {} durable route crash boundaries",
        boundaries.len()
    );
}

#[test]
fn historical_clear_ack_requires_a_fresh_full_vector_ack_after_applied_state_returns() {
    let f = Fixture::new();
    let mut owner = f.owner();
    f.start(&mut owner);
    let directory = owner.directory().unwrap().to_owned();
    let route: Value =
        serde_json::from_slice(&fs::read(directory.join("runtime/routing.json")).unwrap()).unwrap();
    let endpoint = route["endpoint"].as_str().unwrap();
    f.edit(|s| s["reverse"][endpoint] = json!("tcp:external"));
    assert!(matches!(
        owner.stop().unwrap(),
        StopOutcome::RecoveryRequired(_)
    ));
    let saved: Value =
        serde_json::from_slice(&fs::read(directory.join("runtime/routing.json")).unwrap()).unwrap();
    assert_eq!(saved["clear_acknowledged"], true);
    assert_eq!(f.state()["settings"], json!({}));
    f.edit(|s| {
        s["reverse"][endpoint] = json!(endpoint);
        s["settings"] = route["applied"]
            .as_object()
            .unwrap()
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), json!(v))))
            .collect::<serde_json::Map<_, _>>()
            .into();
        s["acknowledge_clear"] = json!(false);
    });
    assert!(matches!(
        owner.recover().unwrap(),
        StopOutcome::RecoveryRequired(_)
    ));
    assert!(listener(&directory));
    assert!(owner.export().is_err());
    f.edit(|s| s["acknowledge_clear"] = json!(true));
    assert!(matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));
    assert_eq!(f.state()["settings"], json!({}));
}

#[test]
fn network_raw_export_streams_regular_files_larger_than_the_old_log_reader_bound() {
    let f = Fixture::new();
    let mut owner = f.owner();
    f.start(&mut owner);
    let path = owner.directory().unwrap().join("network/capture.mitm");
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(35 * 1024 * 1024).unwrap();
    file.sync_all().unwrap();
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    let archive = owner.export().unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    assert_eq!(
        zip.by_name("network/capture.mitm").unwrap().size(),
        35 * 1024 * 1024
    );
}

#[test]
fn missing_engine_is_a_prerequisite_refusal_before_device_mutation() {
    let f = Fixture::new();
    let mut tools = f.tools.clone();
    tools.mitmdump = None;
    let mut owner = CaptureOwner::open(f.paths.clone(), tools, true).unwrap();
    let error = owner
        .start(StartRequest {
            serial: "fixture-stream".into(),
            whole_device_consent: true,
            selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
            limits: Limits::default(),
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        })
        .unwrap_err();
    assert!(error.contains("mitmdump 12.2.3"));
    assert!(owner.directory().is_none());
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
    assert!(f.state()["commands"].as_array().unwrap().is_empty());
}

fn freeze_binaries() -> tempfile::TempDir {
    use sha2::{Digest, Sha256};
    let frozen = tempfile::tempdir().unwrap();
    let mut hashes = serde_json::Map::new();
    for (name, source) in [
        ("capture-guardian", env!("CARGO_BIN_EXE_capture-guardian")),
        ("collector-worker", env!("CARGO_BIN_EXE_collector-worker")),
        ("fake-adb", env!("CARGO_BIN_EXE_fake-adb")),
        ("fake-engine", env!("CARGO_BIN_EXE_fake-engine")),
    ] {
        let target = frozen
            .path()
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        fs::copy(source, &target).unwrap();
        hashes.insert(
            name.into(),
            json!(format!("{:x}", Sha256::digest(fs::read(&target).unwrap()))),
        );
    }
    hashes.insert(
        "owner-test".into(),
        json!(format!(
            "{:x}",
            Sha256::digest(fs::read(std::env::current_exe().unwrap()).unwrap())
        )),
    );
    eprintln!(
        "Immutable crash-matrix binaries {}",
        serde_json::Value::Object(hashes)
    );
    frozen
}

#[test]
fn focused_restore_attachment_records_exit_and_lease_facts_before_the_crash_matrix() {
    let frozen = freeze_binaries();
    for attempt in 0..32 {
        let f = Fixture::from_binaries(Some(frozen.path()));
        let at = if attempt % 2 == 0 { 53 } else { 110 };
        let (mut child, gui, directory) = crash_session(&f, Some(at), false);
        request_stop(&directory);
        drop(gui);
        let status = child.wait().unwrap();
        if status.code() != Some(91) {
            let trace = fs::read_to_string(directory.join("runtime/guardian-events.jsonl"))
                .unwrap_or_default();
            let reply = fs::read_to_string(directory.join("runtime/fixture-attach-reply.json"))
                .unwrap_or_default();
            let preserved = f.root.keep();
            let binaries = frozen.keep();
            panic!("focused attempt {attempt} boundary {at} status={status} preserved={} binaries={} reply={reply} trace={trace}", preserved.display(), binaries.display());
        }
        fs::remove_file(directory.join("runtime/fixture-crash.json")).unwrap();
        let mut owner = f.owner();
        let outcome = owner.recover();
        let trace =
            fs::read_to_string(directory.join("runtime/guardian-events.jsonl")).unwrap_or_default();
        if !matches!(&outcome, Ok(StopOutcome::Ready(_))) {
            drop(owner);
            let preserved = f.root.keep();
            let binaries = frozen.keep();
            panic!(
                "attempt {attempt}, result {outcome:?}, preserved={} binaries={} trace {trace}",
                preserved.display(),
                binaries.display()
            );
        }
        assert!(matches!(owner.snapshot().unwrap(), Snapshot::Stopped(_)));
        let trace = fs::read_to_string(directory.join("runtime/guardian-events.jsonl")).unwrap();
        let records: Vec<Value> = trace
            .rsplit_once('\n')
            .map_or("", |(complete, _)| complete)
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let recovered = records
            .iter()
            .rposition(|r| r["event"] == "guardian_started" && r["recovery"] == true)
            .unwrap();
        assert!(
            records[recovered..]
                .iter()
                .all(|r| r["event"] != "exit_quiescent_no_clients"),
            "attempt {attempt}, trace {trace}"
        );
    }
}

#[test]
fn lost_startup_reply_keeps_same_controller_recovery_and_does_not_hide_mutation() {
    let f = Fixture::new();
    let mut tools = f.tools.clone();
    tools.guardian = env!("CARGO_BIN_EXE_fixture-guardian-drop-reply").into();
    let mut owner = CaptureOwner::open(f.paths.clone(), tools, true).unwrap();
    assert!(owner
        .start(StartRequest {
            serial: "fixture-stream".into(),
            whole_device_consent: true,
            selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
            limits: Limits::default(),
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        })
        .is_err());
    let directory = owner
        .directory()
        .expect("published pending capture must remain attached to its controller")
        .to_owned();
    f.edit(|s| s["connected"] = json!(false));
    wait_until(|| {
        serde_json::from_slice::<Value>(&fs::read(directory.join("session.json")).unwrap()).unwrap()
            ["error"]
            .is_string()
    });
    assert!(matches!(
        owner.snapshot().unwrap(),
        Snapshot::RecoveryRequired(_)
    ));
    assert!(listener(&directory));
    f.edit(|s| s["connected"] = json!(true));
    assert!(matches!(owner.recover().unwrap(), StopOutcome::Ready(_)));
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
}

#[test]
fn completed_session_reopens_and_exports_only_after_reacquiring_sealed_source_proof() {
    let f = Fixture::new();
    let mut owner = f.owner();
    f.start(&mut owner);
    let directory = owner.directory().unwrap().to_owned();
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    drop(owner);
    wait_until(|| {
        fs::read_to_string(directory.join("runtime/guardian-events.jsonl"))
            .unwrap_or_default()
            .contains("exit_quiescent_no_clients")
    });
    fs::remove_file(&f.tools.adb).unwrap();
    let mut reopened = f.owner();
    assert!(matches!(reopened.snapshot().unwrap(), Snapshot::Stopped(_)));
    let archive = match reopened.export() {
        Ok(archive) => archive,
        Err(error) => {
            let trace = fs::read_to_string(directory.join("runtime/guardian-events.jsonl"))
                .unwrap_or_default();
            drop(reopened);
            let kept = f.root.keep();
            panic!(
                "completed export error={error}; preserved={} events={trace}",
                kept.display()
            );
        }
    };
    assert!(archive.is_file());
    let header: Value =
        serde_json::from_slice(&fs::read(directory.join("session.json")).unwrap()).unwrap();
    assert_eq!(header["routing_restored"], true);
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
}

#[test]
fn network_http_tls_and_parse_errors_mark_both_completion_and_archive_partial() {
    for kind in ["error", "tls_error", "parse_error"] {
        let f = Fixture::new();
        let mut owner = f.owner();
        f.start(&mut owner);
        let directory = owner.directory().unwrap().to_owned();
        let event = if kind == "parse_error" {
            b"invalid network record\n".to_vec()
        } else {
            let mut line = serde_json::to_vec(&json!({"timestamp":"2026-10-20T00:00:00Z","source":"network","kind":kind,"summary":"fixture network failure","detail":{},"clock_uncertainty_ms":1})).unwrap();
            line.push(b'\n');
            line
        };
        fs::write(directory.join("network/events.jsonl"), event).unwrap();
        let StopOutcome::Ready(view) = owner.stop().unwrap() else {
            panic!("verified restoration required")
        };
        let NetworkView::Stopped(completion) = view.network else {
            panic!("network completion required")
        };
        assert!(completion.partial, "{kind}");
        assert!(completion
            .reasons
            .iter()
            .any(|reason| reason.contains("Network evidence contains")));
        let mut zip =
            zip::ZipArchive::new(fs::File::open(owner.export().unwrap()).unwrap()).unwrap();
        let mut bytes = String::new();
        zip.by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut bytes)
            .unwrap();
        let manifest: Value = serde_json::from_str(&bytes).unwrap();
        assert_eq!(manifest["package_state"], "partial", "{kind}");
        assert_eq!(manifest["completeness"]["partial"], true);
        let field = if kind == "parse_error" {
            "unparsed_event_refs"
        } else {
            "network_error_refs"
        };
        assert!(manifest["completeness"][field]
            .as_array()
            .unwrap()
            .contains(&json!("network/events.jsonl#L1")));
    }
}

#[test]
fn split_control_frame_survives_nonblocking_listener_accept_before_device_mutation() {
    let f = Fixture::new();
    let (mut child, gui, directory) = crash_session(&f, None, true);
    let reply: Value = serde_json::from_slice(
        &fs::read(directory.join("runtime/fixture-attach-reply.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(reply["Snapshot"]["phase"], "recording");
    assert!(listener(&directory));
    request_stop(&directory);
    drop(gui);
    assert!(child.wait().unwrap().success());
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
    let trace = fs::read_to_string(directory.join("runtime/guardian-events.jsonl")).unwrap();
    assert!(!trace.contains("frame_rejected"), "{trace}");
    assert!(trace.contains("exit_quiescent_no_clients"), "{trace}");
}

#[test]
fn independent_network_selection_omits_logs_without_claiming_missing_evidence() {
    let f = Fixture::new();
    let mut owner = f.owner();
    let policy = NetworkPolicy {
        max_flows: 321,
        duration_ms: 10_000,
        ..NetworkPolicy::default()
    };
    owner
        .start(StartRequest {
            serial: "fixture-stream".into(),
            whole_device_consent: false,
            selection: CaptureSelection::Network(NetworkConsent { whole_device: true }),
            limits: Limits::default(),
            log_scope: LogScope::App {
                application_id: "ignored.when.logs.off".into(),
            },
            network_policy: policy.clone(),
            context: CaptureContext::default(),
        })
        .unwrap();
    let directory = owner.directory().unwrap().to_owned();
    assert!(!directory.join("logs/device.log").exists());
    assert!(!directory.join("receipt.json").exists());
    assert!(listener(&directory));
    let status = owner.status().unwrap();
    assert!(matches!(status,Snapshot::Recording(ref view) if view.events.is_empty()));
    let config: Value =
        serde_json::from_slice(&fs::read(directory.join("runtime/engine-config.json")).unwrap())
            .unwrap();
    assert_eq!(config["policy"]["max_flows"], 321);
    let outcome = owner.stop().unwrap();
    assert!(
        matches!(outcome,StopOutcome::Ready(ref view) if matches!(&view.network,NetworkView::Stopped(n) if !n.partial))
    );
    let archive = owner.export().unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    let mut manifest = String::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["capture_logs"], false);
    assert_eq!(manifest["sources"]["logs"], "not_selected");
    assert_eq!(
        manifest["completeness"]["sources"]["logs"]["requested"],
        false
    );
    assert_eq!(manifest["package_state"], "complete");
    assert_eq!(manifest["completeness"]["missing_artifacts"], json!([]));
    assert!(zip.by_name("logs/device.log").is_err());
    assert!(zip.by_name("capture.json").is_ok());
}
#[test]
fn network_outlives_log_duration_limit_and_app_identity_gap() {
    for app in [false, true] {
        let f = Fixture::new();
        let app_state = f.tools.adb.with_file_name("fake-android-state.json");
        fs::write(&app_state,serde_json::to_vec(&json!({"user":0,"uid":10001,"install":"v1","shared":false,"uid_supported":true,"processes":true})).unwrap()).unwrap();
        let mut owner = f.owner();
        owner
            .start(StartRequest {
                serial: if app {
                    "fixture-app-uid"
                } else {
                    "fixture-stream"
                }
                .into(),
                whole_device_consent: !app,
                selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
                limits: Limits {
                    duration_ms: if app { 5000 } else { 100 },
                    ..Limits::default()
                },
                log_scope: if app {
                    LogScope::App {
                        application_id: "com.fixture.app".into(),
                    }
                } else {
                    LogScope::WholeDevice
                },
                network_policy: NetworkPolicy {
                    duration_ms: 10_000,
                    ..NetworkPolicy::default()
                },
                context: CaptureContext::default(),
            })
            .unwrap();
        let directory = owner.directory().unwrap().to_owned();
        if app {
            wait_until(|| {
                fs::metadata(directory.join("logs/device.log")).is_ok_and(|m| m.len() > 0)
            });
            fs::write(
                &app_state,
                serde_json::to_vec(
                    &json!({"user":0,"uid":10001,"install":"v2","uid_supported":true}),
                )
                .unwrap(),
            )
            .unwrap();
        }
        wait_until(|| directory.join("receipt.json").is_file());
        let receipt: Receipt =
            serde_json::from_slice(&fs::read(directory.join("receipt.json")).unwrap()).unwrap();
        assert_eq!(
            receipt.reason,
            if app {
                StopReason::IdentityGap
            } else {
                StopReason::DurationLimit
            }
        );
        thread::sleep(Duration::from_millis(300));
        assert!(matches!(owner.status().unwrap(), Snapshot::Recording(_)));
        assert!(listener(&directory));
        assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
        assert_eq!(f.state()["settings"], json!({}));
        assert_eq!(f.state()["reverse"], json!({}));
        assert!(owner.export().unwrap().is_file());
    }
}

#[test]
fn completed_control_release_allows_isolated_setup_and_original_export() {
    let f = Fixture::new();
    let mut original = f.owner();
    f.start(&mut original);
    assert!(original.release_completed_control().is_err());
    original.stop().unwrap();
    let original_directory = original.directory().unwrap().to_owned();
    original.release_completed_control().unwrap();
    let mut setup = f.owner();
    setup
        .start(StartRequest {
            serial: "fixture-stream".into(),
            whole_device_consent: false,
            selection: CaptureSelection::Network(NetworkConsent { whole_device: true }),
            limits: Limits::default(),
            log_scope: LogScope::WholeDevice,
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
        })
        .unwrap();
    assert_ne!(setup.directory().unwrap(), original_directory);
    setup.stop().unwrap();
    drop(setup);
    assert!(original.export().unwrap().is_file());
    assert_eq!(original.directory().unwrap(), original_directory);
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
}

#[test]
fn admitted_large_network_record_survives_preview_and_full_export() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let f = Fixture::new();
    let mut owner = f.owner();
    f.start(&mut owner);
    let directory = owner.directory().unwrap().to_owned();
    let body = vec![b'z'; 3 * 1024 * 1024];
    let base64 = STANDARD.encode(&body);
    let record = json!({"timestamp":"2026-10-15T00:00:01.5Z","kind":"response","summary":"large full body","detail":{"response_body":{"text":base64,"encoding":"base64","byte_basis":"raw_content","unavailable":false}},"clock_uncertainty_ms":1});
    fs::write(
        directory.join("network/events.jsonl"),
        format!("{record}\n"),
    )
    .unwrap();
    let live = owner.snapshot().unwrap();
    assert!(
        matches!(live,Snapshot::Recording(ref view) if view.events.iter().any(|e|e.source=="network"))
    );
    owner.stop().unwrap();
    let archive = owner.export().unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    let mut timeline = String::new();
    zip.by_name("timeline/events.jsonl")
        .unwrap()
        .read_to_string(&mut timeline)
        .unwrap();
    let event: Value = timeline
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|v| v["source"] == "network")
        .unwrap();
    let saved = STANDARD
        .decode(event["detail"]["response_body"]["text"].as_str().unwrap())
        .unwrap();
    assert_eq!(saved, body);
    let mut public: Value =
        serde_json::from_slice(&fs::read(directory.join("capture.json")).unwrap()).unwrap();
    public["selection"] = json!({"mode":"network","consent":{"whole_device":true}});
    fs::write(
        directory.join("capture.json"),
        serde_json::to_vec(&public).unwrap(),
    )
    .unwrap();
    assert!(owner.export().unwrap_err().contains("descriptor differs"));
}

#[test]
fn saved_network_selection_uses_private_admission_and_exports_without_device_actions() {
    let f = Fixture::new();
    let mut first = f.owner();
    f.start(&mut first);
    first.stop().unwrap();
    let older = first.directory().unwrap().to_owned();
    first.release_completed_control().unwrap();
    let mut second = f.owner();
    f.start(&mut second);
    second.stop().unwrap();
    let newer = second.directory().unwrap().to_owned();
    drop(second);
    wait_until(|| {
        fs::read_to_string(newer.join("runtime/guardian-events.jsonl"))
            .unwrap_or_default()
            .contains("exit_quiescent_no_clients")
    });
    drop(first);
    fs::remove_file(&f.tools.adb).unwrap();
    let commands = f.state()["commands"].clone();
    let mut reopened = f.owner();
    assert!(matches!(
        reopened.open_saved(older.clone()).unwrap(),
        Snapshot::Stopped(_)
    ));
    assert_eq!(reopened.directory().unwrap(), older);
    assert_eq!(f.state()["commands"], commands);
    assert!(reopened.export().unwrap().is_file());
    assert_eq!(f.state()["commands"], commands);
}

#[test]
fn network_admission_duration_does_not_end_selected_logs_or_routing() {
    let f = Fixture::new();
    let mut owner = f.owner();
    owner
        .start(StartRequest {
            serial: "fixture-stream".into(),
            whole_device_consent: true,
            selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
            limits: Limits {
                duration_ms: 5000,
                ..Limits::default()
            },
            log_scope: LogScope::WholeDevice,
            network_policy: NetworkPolicy {
                duration_ms: 100,
                ..NetworkPolicy::default()
            },
            context: CaptureContext::default(),
        })
        .unwrap();
    let directory = owner.directory().unwrap().to_owned();
    wait_until(
        || matches!(owner.status().unwrap(),Snapshot::Recording(ref view) if matches!(&view.network,NetworkView::Recording(coverage) if coverage.admission_closed)),
    );
    let before = fs::metadata(directory.join("logs/device.log"))
        .unwrap()
        .len();
    thread::sleep(Duration::from_millis(300));
    assert!(
        fs::metadata(directory.join("logs/device.log"))
            .unwrap()
            .len()
            > before
    );
    assert!(!directory.join("receipt.json").exists());
    assert!(listener(&directory));
    assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
}
