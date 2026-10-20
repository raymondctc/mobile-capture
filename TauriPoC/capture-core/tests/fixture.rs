use std::process::Command;

#[test]
fn compiled_adb_fixture_lists_real_discovery_states() {
    let result = Command::new(env!("CARGO_BIN_EXE_fake-adb"))
        .args(["devices", "-l"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("fixture-stream\tdevice"));
    assert!(text.contains("fixture-offline\toffline"));
    assert!(text.contains("fixture-denied\tunauthorized"));
}
