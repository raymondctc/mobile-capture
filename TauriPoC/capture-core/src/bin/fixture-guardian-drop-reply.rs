use std::path::PathBuf;
fn main() {
    let path = PathBuf::from(std::env::args_os().nth(1).expect("private fixture startup"));
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(config["fixture"], true);
    config["fixture_drop_initial_reply"] = serde_json::json!(true);
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    if let Err(error) = capture_poc_core::run_guardian(&path) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
