use base64::{engine::general_purpose::STANDARD, Engine};
use capture_poc_core::{
    model::*,
    setup::{identity, CheckDetails, SetupCapture},
    CaptureOwner,
};
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
    _root: tempfile::TempDir,
    tools: ToolSelection,
    paths: CapturePaths,
    state: PathBuf,
    batch: u64,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let tools_dir = root.path().join("tools");
        fs::create_dir(&tools_dir).unwrap();
        let copy = |source: &str, name: &str| {
            let target = tools_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            fs::copy(source, &target).unwrap();
            target
        };
        let tools = ToolSelection {
            adb: copy(env!("CARGO_BIN_EXE_fake-adb"), "fake-adb"),
            worker: copy(env!("CARGO_BIN_EXE_collector-worker"), "collector-worker"),
            guardian: copy(env!("CARGO_BIN_EXE_capture-guardian"), "capture-guardian"),
            mitmdump: Some(copy(env!("CARGO_BIN_EXE_fake-engine"), "fake-engine")),
            addon: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../resources/network_addon.py")
                .canonicalize()
                .unwrap(),
        };
        let state = tools_dir.join("fake-adb-state.json");
        fs::write(
            &state,
            serde_json::to_vec(&json!({"settings":{},"reverse":{},"commands":[]})).unwrap(),
        )
        .unwrap();
        let paths = CapturePaths {
            output_root: root.path().join("output"),
            runtime_root: Some(root.path().join("private-namespace")),
        };
        Self {
            _root: root,
            tools,
            paths,
            state,
            batch: 0,
        }
    }

    fn setup(&self) -> SetupCapture {
        SetupCapture::open(
            self.paths.clone(),
            self.tools.clone(),
            true,
            "fixture-stream".into(),
        )
        .unwrap()
    }

    fn owner(&self) -> CaptureOwner {
        CaptureOwner::open(self.paths.clone(), self.tools.clone(), true).unwrap()
    }

    fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.state).unwrap()).unwrap()
    }

    fn edit(&self, edit: impl FnOnce(&mut Value)) {
        let mut state = self.state();
        edit(&mut state);
        let pending = self.state.with_extension("test-pending");
        fs::write(&pending, serde_json::to_vec(&state).unwrap()).unwrap();
        fs::rename(pending, &self.state).unwrap();
    }

    fn emit(&mut self, directory: &Path, records: Vec<String>) {
        self.batch += 1;
        let control = self
            .tools
            .mitmdump
            .as_ref()
            .unwrap()
            .with_file_name("fake-engine-control.json");
        let pending = control.with_extension("pending");
        fs::write(
            &pending,
            serde_json::to_vec(
                &json!({"event_batch":{"id":self.batch,"directory":directory,"records":records}}),
            )
            .unwrap(),
        )
        .unwrap();
        fs::rename(pending, control).unwrap();
        wait_until(|| {
            fs::read_to_string(directory.join("runtime/fake-engine-events.json"))
                .ok()
                .as_deref()
                == Some(&self.batch.to_string())
        });
    }

    fn assert_main_capture_after(&self, setup_directory: &Path) {
        let mut owner = self.owner();
        let snapshot = owner.start(main_request()).unwrap();
        let Snapshot::Recording(session) = snapshot else {
            panic!("main capture must start after setup restoration")
        };
        assert_ne!(session.directory, setup_directory);
        assert!(matches!(
            session.selection,
            CaptureSelection::LogsAndNetwork(_)
        ));
        assert!(matches!(owner.stop().unwrap(), StopOutcome::Ready(_)));
        assert_eq!(self.state()["reverse"], json!({}));
    }
}

fn main_request() -> StartRequest {
    StartRequest {
        serial: "fixture-stream".into(),
        whole_device_consent: true,
        selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
        limits: Limits::default(),
        log_scope: LogScope::WholeDevice,
        network_policy: NetworkPolicy::default(),
        context: CaptureContext::default(),
    }
}

fn wait_until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !check() {
        assert!(Instant::now() < deadline, "fixture operation timed out");
        thread::sleep(Duration::from_millis(20));
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

fn probe(details: &CheckDetails) -> Value {
    let nonce = details
        .url
        .strip_prefix("https://mitm.it/mobile-capture-check/")
        .unwrap()
        .strip_suffix("/https")
        .unwrap();
    let payload = serde_json::to_vec(&json!({"nonce":nonce,"capture_check":"browser_https","message":"Synthetic capture check: café 日本語"})).unwrap();
    let received = now();
    json!({"id":"fixture-setup-response","timestamp":received,"received_at":received,"source":"network","kind":"response","summary":"fixture setup HTTPS response","raw_ref":"network/events.jsonl","clock_uncertainty_ms":0,"detail":{
        "url":details.url,"method":"GET","status":200,"setup_probe":true,"setup_probe_nonce":nonce,"setup_probe_transport":"android_reverse",
        "response_body":{"encoding":"base64","text":STANDARD.encode(&payload),"bytes":payload.len(),"byte_basis":"raw_content"}
    }})
}

fn line(value: &Value) -> String {
    format!("{value}\n")
}

#[test]
fn certificate_handoff_is_public_staged_network_only_and_restores_exact_routing() {
    let f = Fixture::new();
    let original = json!({"http_proxy":"null","global_http_proxy_host":"","global_http_proxy_exclusion_list":"a=b 'quoted'","global_proxy_pac_url":""});
    f.edit(|state| state["settings"] = original.clone());
    assert!(identity(&f.paths, true)
        .unwrap()
        .certificate_fingerprint
        .is_none());
    let mut setup = f.setup();
    let details = setup.start().unwrap();
    assert!(listener(&details.directory));
    assert!(!details.directory.join("logs/device.log").exists());
    assert!(!details.directory.join("receipt.json").exists());
    let public = fs::read_to_string(&details.certificate.path).unwrap();
    fs::write(
        &details.certificate.path,
        format!("{public}-----BEGIN PRIVATE KEY-----\nfixture-secret\n-----END PRIVATE KEY-----\n"),
    )
    .unwrap();
    assert!(setup.copy_public_certificate().is_err());
    assert!(f.state()["pushed_certificates"]
        .as_object()
        .unwrap()
        .is_empty());
    fs::write(&details.certificate.path, &public).unwrap();
    let target = setup.copy_public_certificate().unwrap();
    assert_eq!(
        target,
        format!(
            "/sdcard/Download/mobile-capture-ca-{}.crt",
            &details
                .certificate
                .fingerprint
                .replace(':', "")
                .to_lowercase()[..12]
        )
    );
    assert_eq!(
        f.state()["pushed_certificates"],
        json!({target.clone():public})
    );
    let state = f.state();
    let pushes: Vec<_> = state["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|command| command[0] == "push")
        .collect();
    assert_eq!(pushes.len(), 1);
    let stage = Path::new(pushes[0][1].as_str().unwrap());
    assert_ne!(stage, details.certificate.path);
    assert!(
        !stage.exists(),
        "public staging file must be removed after ADB returns"
    );
    assert!(!state.to_string().contains("PRIVATE KEY"));
    let current = identity(&f.paths, true).unwrap();
    assert_eq!(
        current.certificate_fingerprint.as_deref(),
        Some(details.certificate.fingerprint.as_str())
    );
    assert!(setup.stop().unwrap());
    assert_eq!(f.state()["settings"], original);
    assert_eq!(f.state()["reverse"], json!({}));
    assert!(!listener(&details.directory));
    drop(setup);
    let mut saved = f.owner();
    saved.open_saved(details.directory.clone()).unwrap();
    let archive = saved.export().unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    let mut manifest = String::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["sources"]["logs"], "not_selected");
    assert_eq!(
        manifest["completeness"]["sources"]["logs"]["requested"],
        false
    );
    assert_eq!(manifest["capture_logs"], false);
    assert_eq!(manifest["routing_restored"], true);
    assert!(zip.by_name("logs/device.log").is_err());
    drop(zip);
    drop(saved);
    f.assert_main_capture_after(&details.directory);
    assert_eq!(f.state()["settings"], original);
}

#[test]
fn only_complete_fresh_owned_https_probe_is_observed_and_stop_recovery_retains_owner() {
    let mut f = Fixture::new();
    let mut setup = f.setup();
    let details = setup.start().unwrap();
    assert!(!setup.observe_https().unwrap());
    setup.open_browser().unwrap();
    assert!(f.state()["commands"].as_array().unwrap().contains(&json!([
        "shell",
        "am",
        "start",
        "-a",
        "android.intent.action.VIEW",
        "-d",
        details.url
    ])));
    assert!(
        !setup.observe_https().unwrap(),
        "opening the browser is not HTTPS evidence"
    );
    let valid = probe(&details);
    for (pointer, value) in [
        ("/source", json!("logs")),
        ("/kind", json!("tls_error")),
        ("/detail/status", json!(201)),
        ("/detail/url", json!(details.url.replace("https:", "http:"))),
        ("/detail/url", json!(format!("{}?other", details.url))),
        ("/detail/method", json!("POST")),
        ("/detail/setup_probe", json!(false)),
        ("/detail/setup_probe_nonce", json!("00".repeat(16))),
        ("/detail/setup_probe_transport", json!("host")),
        ("/detail/response_body/encoding", json!("utf8")),
        ("/detail/response_body/byte_basis", json!("content")),
        ("/detail/response_body/text", json!("not base64")),
        ("/detail/response_body/bytes", json!(1)),
        ("/received_at", json!("2000-01-01T00:00:00Z")),
        ("/received_at", json!("2999-01-01T00:00:00Z")),
        ("/received_at", Value::Null),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        f.emit(&details.directory, vec![line(&invalid)]);
        assert!(
            !setup.observe_https().unwrap(),
            "accepted altered probe {pointer}"
        );
    }
    for field in ["preview", "unavailable"] {
        let mut invalid = valid.clone();
        invalid["detail"]["response_body"][field] = json!(true);
        f.emit(&details.directory, vec![line(&invalid)]);
        assert!(
            !setup.observe_https().unwrap(),
            "accepted incomplete body {field}"
        );
    }
    for payload in [
        json!({"nonce":"wrong","capture_check":"browser_https","message":"Synthetic capture check: café 日本語"}),
        json!({"nonce":valid["detail"]["setup_probe_nonce"],"capture_check":"browser_http","message":"Synthetic capture check: café 日本語"}),
        json!({"nonce":valid["detail"]["setup_probe_nonce"],"capture_check":"browser_https","message":"wrong"}),
    ] {
        let mut invalid = valid.clone();
        let bytes = serde_json::to_vec(&payload).unwrap();
        invalid["detail"]["response_body"]["text"] = json!(STANDARD.encode(&bytes));
        invalid["detail"]["response_body"]["bytes"] = json!(bytes.len());
        f.emit(&details.directory, vec![line(&invalid)]);
        assert!(
            !setup.observe_https().unwrap(),
            "accepted another synthetic payload"
        );
    }
    f.emit(
        &details.directory,
        vec!["malformed fixture record\n".into(), valid.to_string()],
    );
    assert!(
        !setup.observe_https().unwrap(),
        "unfinished JSONL record is not evidence"
    );
    f.emit(&details.directory, vec!["\n".into()]);
    assert!(setup.observe_https().unwrap());
    assert!(setup.observe_https().unwrap());
    f.edit(|state| state["connected"] = json!(false));
    assert!(!setup.stop().unwrap());
    assert_eq!(
        setup.directory().as_deref(),
        Some(details.directory.as_path())
    );
    assert!(listener(&details.directory));
    assert!(!setup.recover().unwrap());
    assert_eq!(
        setup.directory().as_deref(),
        Some(details.directory.as_path())
    );
    let mut blocked = f.owner();
    assert!(blocked.start(main_request()).is_err());
    assert_eq!(blocked.directory(), Some(details.directory.as_path()));
    assert!(listener(&details.directory));
    drop(blocked);
    f.edit(|state| state["connected"] = json!(true));
    assert!(setup.recover().unwrap());
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
    assert!(!listener(&details.directory));
    drop(setup);
    f.assert_main_capture_after(&details.directory);
}

#[test]
fn failed_setup_start_keeps_directory_for_recovery_and_later_main_capture() {
    let mut f = Fixture::new();
    let wrapper = f.tools.guardian.with_file_name(format!(
        "fixture-guardian-drop-reply{}",
        std::env::consts::EXE_SUFFIX
    ));
    fs::copy(env!("CARGO_BIN_EXE_fixture-guardian-drop-reply"), &wrapper).unwrap();
    f.tools.guardian = wrapper;
    let mut setup = f.setup();
    assert!(setup.start().is_err());
    let directory = setup
        .directory()
        .expect("a published failed start retains its owner");
    f.edit(|state| state["connected"] = json!(false));
    wait_until(|| {
        fs::read(directory.join("session.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|header| header["error"].is_string())
    });
    assert!(listener(&directory));
    assert!(!setup.recover().unwrap());
    assert_eq!(setup.directory(), Some(directory.clone()));
    f.edit(|state| state["connected"] = json!(true));
    assert!(setup.recover().unwrap());
    assert_eq!(f.state()["settings"], json!({}));
    assert_eq!(f.state()["reverse"], json!({}));
    assert!(!listener(&directory));
    drop(setup);
    f.tools.guardian = f
        .tools
        .guardian
        .with_file_name(format!("capture-guardian{}", std::env::consts::EXE_SUFFIX));
    f.assert_main_capture_after(&directory);
}
