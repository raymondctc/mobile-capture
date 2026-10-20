use capture_poc_core::{model::Limits, CaptureOwner};
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
    let mut owner = CaptureOwner::new(
        binaries.join(format!("fake-adb{suffix}")),
        binaries.join(format!("collector-worker{suffix}")),
        root,
        true,
    );
    owner
        .start("fixture-stream".into(), true, Limits::default())
        .unwrap();
    println!("{}", owner.directory().unwrap().display());
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read(&mut [0]);
    // Deliberately bypass Drop to test OS closure of the lease and Windows Job handle.
    std::process::exit(0);
}
