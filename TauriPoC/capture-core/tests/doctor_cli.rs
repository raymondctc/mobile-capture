use serde_json::Value;
use std::process::Command;

fn invoke(args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_android-doctor"))
        .args(args)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.ends_with('\n'));
    assert_eq!(text.lines().count(), 1);
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["deviceSetup"]["status"], "notChecked");
    (output.status.code().unwrap(), json)
}

#[test]
fn rejects_unknown_duplicate_missing_and_relative_arguments_as_json() {
    for arguments in [
        vec![],
        vec!["--json", "--unknown"],
        vec!["--json", "--json"],
        vec!["--json", "--storage-dir"],
        vec!["--json", "--storage-dir", "relative"],
        vec!["--json", "--sources", "all"],
        vec!["--json", "--sources", "logs", "--sources", "network"],
    ] {
        let (code, json) = invoke(&arguments);
        assert_eq!(code, 64, "{arguments:?}");
        assert_eq!(json["error"]["code"], "invalidArguments");
        assert_eq!(json["hostPrerequisitesReady"], false);
    }
}

#[test]
fn missing_installed_payload_never_launches_phone_tools() {
    let resources = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let (code, json) = invoke(&[
        "--json",
        "--resource-dir",
        resources.path().to_str().unwrap(),
        "--storage-dir",
        storage.path().to_str().unwrap(),
    ]);
    assert_eq!(code, 10);
    assert_eq!(json["error"]["code"], "payloadIntegrity");
    assert!(json["checks"].as_array().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(storage.path()).unwrap().count(), 0);
}
