use capture_poc_core::{model::*, CaptureOwner};
use serde::Deserialize;
use std::{
    io::{Read, Write},
    path::PathBuf,
};
#[derive(Deserialize)]
struct Config {
    paths: CapturePaths,
    tools: ToolSelection,
    serial: Option<String>,
}
fn main() {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("private fixture configuration"),
    );
    let config: Config = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut owner = CaptureOwner::open(config.paths, config.tools, true).unwrap();
    let snapshot = owner
        .start(StartRequest {
            serial: config.serial.unwrap_or_else(|| "fixture-stream".into()),
            whole_device_consent: true,
            selection: CaptureSelection::LogsAndNetwork(NetworkConsent { whole_device: true }),
            limits: Limits::default(),
        })
        .unwrap();
    println!("{}", serde_json::to_string(&snapshot).unwrap());
    std::io::stdout().flush().unwrap();
    let mut action = [0];
    let _ = std::io::stdin().read(&mut action);
    if action == [b'e'] {
        let outcome = owner.stop().unwrap();
        let archive = owner.export().unwrap();
        println!(
            "{}",
            serde_json::json!({"outcome":outcome,"archive":archive})
        );
        return;
    }
    std::process::exit(0);
}
