#![cfg(unix)]

use capture_poc_core::model::{Limits, Receipt, StopReason, WorkerConfig};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::Shutdown,
    os::unix::net::{UnixListener, UnixStream},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Worker {
    child: Child,
    control: Option<UnixStream>,
    descendant: Option<u32>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.shutdown(Shutdown::Both);
        }
        if let Some(descendant) = self.descendant.take() {
            unsafe { libc::kill(descendant as i32, libc::SIGKILL) };
        }
        self.child.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn process_state(pid: u32) -> String {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    assert!(output.status.success() || output.status.code() == Some(1));
    String::from_utf8(output.stdout).unwrap().trim().into()
}

#[test]
fn stop_terminates_inherited_pipes_after_the_group_leader_exits() {
    check_descendants("fixture-descendant", false);
}

#[test]
fn stop_kills_sigterm_resistant_descendants_after_the_group_leader_exits() {
    check_descendants("fixture-descendant-ignore-term", true);
}

#[test]
fn stop_kills_sigterm_resistant_descendants_that_retain_only_stderr() {
    check_descendants("fixture-descendant-ignore-term-stderr-only", true);
}

#[test]
fn stop_drains_stderr_after_the_diagnostic_storage_cap() {
    check_descendants(
        "fixture-descendant-ignore-term-stderr-only-stderr-cap",
        true,
    );
}

fn check_descendants(prefix: &str, forced: bool) {
    for (serial, exit_code) in [
        (format!("{prefix}-clean"), Some(0)),
        (format!("{prefix}-error"), Some(23)),
        (format!("{prefix}-signal"), None),
    ] {
        let root = tempfile::tempdir().unwrap();
        let control_path = root.path().join("control.sock");
        let listener = UnixListener::bind(&control_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut worker = Worker {
            child: Command::new(env!("CARGO_BIN_EXE_collector-worker"))
                .env("FIXTURE_GROUP_CONTROL", &control_path)
                .stdin(Stdio::piped())
                .spawn()
                .unwrap(),
            control: None,
            descendant: None,
        };
        let config = WorkerConfig {
            source_lease: false,
            adb: env!("CARGO_BIN_EXE_fake-adb").into(),
            serial: serial.clone(),
            directory: root.path().into(),
            limits: Limits::default(),
        };
        writeln!(
            worker.child.stdin.as_mut().unwrap(),
            "{}",
            serde_json::to_string(&config).unwrap()
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            match listener.accept() {
                Ok((control, _)) => {
                    control.set_nonblocking(false).unwrap();
                    worker.control = Some(control);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("Cannot accept fixture control: {error}"),
            }
            assert!(Instant::now() < deadline, "Fixture did not connect");
            thread::sleep(Duration::from_millis(10));
        }
        let control = worker.control.as_mut().unwrap();
        control
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut ready = String::new();
        BufReader::new(control.try_clone().unwrap())
            .read_line(&mut ready)
            .unwrap();
        let pids: Vec<u32> = ready
            .split_whitespace()
            .map(|pid| pid.parse().unwrap())
            .collect();
        assert_eq!(pids.len(), 2);
        let (leader, descendant) = (pids[0], pids[1]);
        worker.descendant = Some(descendant);
        while !process_state(leader).starts_with('Z') {
            assert!(Instant::now() < deadline, "ADB leader did not exit");
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(unsafe { libc::kill(descendant as i32, 0) }, 0);
        if serial.contains("stderr-only") {
            control.write_all(&[1]).unwrap();
            let mut closed = String::new();
            BufReader::new(control.try_clone().unwrap())
                .read_line(&mut closed)
                .unwrap();
            assert_eq!(closed, "stdout closed\n");
        }
        worker.child.stdin.take();
        let closed = control.read(&mut [0]);
        assert!(
            matches!(closed, Ok(0)),
            "Owned descendant survived Stop and retained its pipes: {closed:?}"
        );
        loop {
            let state = process_state(descendant);
            if state.is_empty() || state.starts_with('Z') {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Descendant remains alive: {state}"
            );
            thread::sleep(Duration::from_millis(10));
        }
        worker.descendant = None;
        loop {
            if let Some(status) = worker.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "Worker did not finish Stop");
            thread::sleep(Duration::from_millis(10));
        }
        let receipt: Receipt =
            serde_json::from_slice(&fs::read(root.path().join("receipt.json")).unwrap()).unwrap();
        assert_eq!(receipt.reason, StopReason::CollectorExit, "{serial}");
        assert_eq!(receipt.exit_code, exit_code, "{serial}");
        assert!(receipt.child_reaped);
        assert_eq!(receipt.forced, forced, "{serial}");
        if serial.contains("stderr-only") {
            let mut expected = "fixture descendant retains stderr\n".to_string();
            if serial.contains("stderr-cap") {
                expected.push_str(&"x".repeat(8192 - expected.len()));
            }
            assert_eq!(receipt.stderr, expected);
        }
        assert_eq!(
            fs::read(root.path().join("logs/device.log")).unwrap(),
            b"1792022401.125 123 456 I Fixture: inherited pipes\n"
        );
    }
}
