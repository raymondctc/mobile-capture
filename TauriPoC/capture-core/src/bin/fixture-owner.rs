use capture_poc_core::{model::*, CaptureOwner};
use std::{
    io::{Read, Write},
    path::PathBuf,
};
fn main() {
    let root = PathBuf::from(std::env::args_os().nth(1).expect("fixture output root"));
    let binaries = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let mut owner = CaptureOwner::open(
        CapturePaths {
            output_root: root.clone(),
            runtime_root: Some(root.join("fixture-runtime")),
        },
        ToolSelection {
            adb: binaries.join(format!("fake-adb{suffix}")),
            worker: binaries.join(format!("collector-worker{suffix}")),
            guardian: PathBuf::new(),
            mitmdump: None,
            addon: PathBuf::new(),
        },
        true,
    )
    .unwrap();
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
        .unwrap();
    println!("{}", owner.directory().unwrap().display());
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read(&mut [0]);
    std::process::exit(0);
}
