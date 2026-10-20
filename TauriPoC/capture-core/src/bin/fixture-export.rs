use capture_poc_core::{model::Limits, CaptureOwner};
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
fn main() {
    let root = PathBuf::from(std::env::args_os().nth(1).expect("fixture output root"));
    let binaries = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let mut owner = CaptureOwner::new(
        binaries.join(format!("fake-adb{suffix}")),
        binaries.join(format!("collector-worker{suffix}")),
        root,
        true,
    );
    owner
        .start("fixture-stream".into(), true, Limits::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while std::fs::metadata(owner.directory().unwrap().join("logs/device.log"))
        .map(|m| m.len())
        .unwrap_or(0)
        < 100
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(30));
    }
    owner
        .marker("fixture marker \"quoted\"\nsecond line".into())
        .unwrap();
    owner.stop().unwrap();
    println!("{}", owner.export().unwrap().display());
}
