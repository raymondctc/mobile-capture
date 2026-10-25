fn main() {
    println!("cargo:rerun-if-env-changed=MOBILE_CAPTURE_UPDATER_PUBLIC_KEY");
    println!("cargo:rerun-if-changed=../package.json");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    let package: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("../package.json").unwrap()).unwrap();
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("tauri.conf.json").unwrap()).unwrap();
    assert_eq!(
        package["version"].as_str(),
        Some(env!("CARGO_PKG_VERSION")),
        "Cargo and npm versions disagree"
    );
    assert_eq!(
        package["version"], config["version"],
        "Tauri and npm versions disagree"
    );
    tauri_build::build()
}
