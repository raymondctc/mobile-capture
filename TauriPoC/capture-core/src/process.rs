use crate::model::Result;
use std::{
    io::{self, Read},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

pub fn quiet(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

#[derive(Debug)]
pub(crate) enum OutputError {
    Io(io::Error),
    TimedOut,
    Cancelled,
    Limit { stream: &'static str, bytes: usize },
}

impl From<io::Error> for OutputError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for OutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::TimedOut => f.write_str("Owned command exceeded its deadline"),
            Self::Cancelled => f.write_str("Owned command was cancelled"),
            Self::Limit { stream, bytes } => {
                write!(f, "Owned command {stream} exceeded {bytes} bytes")
            }
        }
    }
}

pub(crate) fn output_bounded(
    command: &mut Command,
    timeout: Duration,
    cancelled: Option<&AtomicBool>,
    stdout_limit: usize,
    stderr_limit: usize,
) -> std::result::Result<Output, OutputError> {
    let deadline = Instant::now() + timeout;
    probe_active(deadline, cancelled)?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = (|| {
        let mut stdout = child.stdout.take().expect("piped command stdout");
        let mut stderr = child.stderr.take().expect("piped command stderr");
        stdout.prepare()?;
        stderr.prepare()?;
        let mut output = Vec::new();
        let mut errors = Vec::new();
        loop {
            probe_active(deadline, cancelled)?;
            let status = child.try_wait()?;
            drain_pipe(
                &mut stdout,
                &mut output,
                stdout_limit,
                "stdout",
                deadline,
                cancelled,
            )?;
            drain_pipe(
                &mut stderr,
                &mut errors,
                stderr_limit,
                "stderr",
                deadline,
                cancelled,
            )?;
            if let Some(status) = status {
                return Ok(Output {
                    status,
                    stdout: output,
                    stderr: errors,
                });
            }
            thread::sleep(
                Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn probe_active(
    deadline: Instant,
    cancelled: Option<&AtomicBool>,
) -> std::result::Result<(), OutputError> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(OutputError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(OutputError::TimedOut);
    }
    Ok(())
}

fn drain_pipe(
    pipe: &mut impl ProbePipe,
    bytes: &mut Vec<u8>,
    limit: usize,
    stream: &'static str,
    deadline: Instant,
    cancelled: Option<&AtomicBool>,
) -> std::result::Result<(), OutputError> {
    let mut buffer = [0; 8192];
    loop {
        probe_active(deadline, cancelled)?;
        let capacity = buffer
            .len()
            .min(limit.saturating_sub(bytes.len()).saturating_add(1));
        match pipe.read_available(&mut buffer[..capacity]) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if count > limit - bytes.len() {
                    return Err(OutputError::Limit {
                        stream,
                        bytes: limit,
                    });
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
}

trait ProbePipe: Read {
    fn prepare(&self) -> io::Result<()>;
    fn read_available(&mut self, bytes: &mut [u8]) -> io::Result<usize>;
}

#[cfg(unix)]
impl<T: Read + std::os::fd::AsRawFd> ProbePipe for T {
    fn prepare(&self) -> io::Result<()> {
        let fd = self.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn read_available(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.read(bytes)
    }
}

#[cfg(windows)]
impl<T: Read + std::os::windows::io::AsRawHandle> ProbePipe for T {
    fn prepare(&self) -> io::Result<()> {
        Ok(())
    }

    fn read_available(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        use windows_sys::Win32::{Foundation::ERROR_BROKEN_PIPE, System::Pipes::PeekNamedPipe};
        let mut available = 0;
        let success = unsafe {
            PeekNamedPipe(
                self.as_raw_handle() as _,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if success == 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                Ok(0)
            } else {
                Err(error)
            };
        }
        if available == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let capacity = bytes.len().min(available as usize);
        self.read(&mut bytes[..capacity])
    }
}

pub fn wait_bounded(child: &mut Child, timeout: Duration) -> Result<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err("Owned process did not exit before the deadline".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub fn discover(adb: &std::path::Path) -> Result<Vec<crate::model::Device>> {
    use crate::model::{Device, DeviceState};
    let mut command = Command::new(adb);
    quiet(&mut command);
    command.args(["devices", "-l"]);
    let output =
        output_bounded(&mut command, Duration::from_secs(5), None, 65536, 65536).map_err(|e| {
            format!("Cannot run ADB. Install Android platform-tools and put adb on PATH. {e}")
        })?;
    if !output.status.success() {
        return Err(format!(
            "ADB discovery failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "ADB discovery was not UTF-8")?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            let state = fields.next()?;
            if serial == "List" || serial.starts_with('*') {
                return None;
            }
            let name = fields
                .find_map(|field| field.strip_prefix("model:"))
                .unwrap_or(serial)
                .replace('_', " ");
            Some(Device {
                serial: serial.into(),
                name,
                state: match state {
                    "device" => DeviceState::Ready,
                    "unauthorized" => DeviceState::Unauthorized,
                    "offline" => DeviceState::Offline,
                    _ => DeviceState::Unavailable,
                },
            })
        })
        .collect())
}

#[cfg(windows)]
pub struct Job(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for Job {}
#[cfg(windows)]
impl Job {
    pub fn contain(child: &Child) -> Result<Self> {
        use std::{
            mem::{size_of, zeroed},
            os::windows::io::AsRawHandle,
        };
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
                || AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0
            {
                let error = std::io::Error::last_os_error().to_string();
                CloseHandle(job);
                return Err(format!(
                    "Cannot contain worker in a kill-on-close Job Object: {error}"
                ));
            }
            Ok(Self(job))
        }
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(not(windows))]
pub struct Job;
#[cfg(not(windows))]
impl Job {
    pub fn contain(_: &Child) -> Result<Self> {
        Ok(Self)
    }
}

pub(crate) fn absolute_executable(path: &std::path::Path) -> Result<std::path::PathBuf> {
    let candidates = if path.components().count() > 1 || path.is_absolute() {
        vec![path.to_path_buf()]
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .flat_map(|directory| {
                #[cfg(windows)]
                let names = vec![
                    directory.join(path),
                    directory.join(path).with_extension("exe"),
                ];
                #[cfg(not(windows))]
                let names = vec![directory.join(path)];
                names
            })
            .collect()
    };
    for candidate in candidates {
        if let Ok(canonical) = std::fs::canonicalize(candidate) {
            if let Ok(metadata) = canonical.metadata() {
                if !metadata.is_file() {
                    continue;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o111 == 0 {
                        continue;
                    }
                }
                return Ok(canonical);
            }
        }
    }
    Err(format!(
        "Cannot resolve executable {} from PATH",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write, path::Path, sync::Arc};

    const FIXTURE: &str = "CAPTURE_BOUNDED_PROBE_FIXTURE";

    fn fixture_command(directory: &Path, mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        quiet(&mut command);
        command
            .args([
                "--exact",
                "process::tests::pipe_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env(FIXTURE, directory)
            .env("CAPTURE_BOUNDED_PROBE_MODE", mode);
        command
    }

    fn wait_file(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "fixture did not create {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn process_exited(pid: u32) -> bool {
        #[cfg(unix)]
        {
            let result = unsafe { libc::kill(pid as i32, 0) };
            result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::{
                Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, STILL_ACTIVE},
                System::Threading::*,
            };
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if handle.is_null() {
                assert_eq!(
                    io::Error::last_os_error().raw_os_error(),
                    Some(ERROR_INVALID_PARAMETER as i32)
                );
                return true;
            }
            let mut status = 0;
            let result = unsafe { GetExitCodeProcess(handle, &mut status) };
            unsafe {
                CloseHandle(handle);
            }
            assert_ne!(result, 0);
            status != STILL_ACTIVE as u32
        }
    }

    struct Fixture(tempfile::TempDir);

    impl Fixture {
        fn new() -> Self {
            Self(tempfile::tempdir().unwrap())
        }

        fn command(&self, mode: &str) -> Command {
            fixture_command(self.0.path(), mode)
        }

        fn assert_owned_child_reaped(&self) {
            let pid: u32 = fs::read_to_string(self.0.path().join("parent.pid"))
                .unwrap()
                .parse()
                .unwrap();
            #[cfg(unix)]
            {
                let mut status = 0;
                assert_eq!(
                    unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
                    -1
                );
                assert_eq!(
                    io::Error::last_os_error().raw_os_error(),
                    Some(libc::ECHILD)
                );
            }
            #[cfg(windows)]
            assert!(process_exited(pid));
        }

        fn assert_descendant_alive(&self) {
            let heartbeat = self.0.path().join("heartbeat");
            let before = fs::read(&heartbeat).unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            while fs::read(&heartbeat).unwrap() == before {
                assert!(
                    Instant::now() < deadline,
                    "probe killed the inherited-pipe descendant"
                );
                thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::write(self.0.path().join("release"), b"").unwrap();
            if self.0.path().join("ready").exists() {
                wait_file(&self.0.path().join("done"));
                let pid = fs::read_to_string(self.0.path().join("descendant.pid"))
                    .unwrap()
                    .parse()
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(3);
                while !process_exited(pid) {
                    assert!(
                        Instant::now() < deadline,
                        "fixture descendant did not exit after release"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    #[test]
    #[ignore = "subprocess fixture entry point"]
    fn pipe_fixture() {
        let Ok(directory) = std::env::var(FIXTURE) else {
            return;
        };
        let directory = Path::new(&directory);
        let mode = std::env::var("CAPTURE_BOUNDED_PROBE_MODE").unwrap();
        if mode == "descendant" {
            fs::write(
                directory.join("descendant.pid"),
                std::process::id().to_string(),
            )
            .unwrap();
            fs::write(directory.join("ready"), b"").unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut beat = 0u64;
            while !directory.join("release").exists() && Instant::now() < deadline {
                beat += 1;
                fs::write(directory.join("heartbeat"), beat.to_le_bytes()).unwrap();
                thread::sleep(Duration::from_millis(10));
            }
            fs::write(directory.join("done"), b"").unwrap();
            std::process::exit(0);
        }
        fs::write(directory.join("parent.pid"), std::process::id().to_string()).unwrap();
        if mode == "bounded" {
            io::stdout().write_all(&vec![b'x'; 96 * 1024]).unwrap();
            io::stderr().write_all(&vec![b'y'; 96 * 1024]).unwrap();
            std::process::exit(0);
        }
        if mode == "stdout-overflow" || mode == "stderr-overflow" {
            let bytes = vec![b'x'; 128 * 1024];
            if mode == "stdout-overflow" {
                let _ = io::stdout().write_all(&bytes);
            } else {
                let _ = io::stderr().write_all(&bytes);
            }
            std::process::exit(0);
        }
        let mut descendant = fixture_command(directory, "descendant")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        wait_file(&directory.join("ready"));
        wait_file(&directory.join("heartbeat"));
        io::stdout().write_all(b"parent stdout\n").unwrap();
        io::stdout().flush().unwrap();
        io::stderr().write_all(b"parent stderr\n").unwrap();
        io::stderr().flush().unwrap();
        if mode == "exit" {
            std::process::exit(0);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while !directory.join("release").exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        descendant.wait().unwrap();
        std::process::exit(0);
    }

    #[test]
    fn deadline_closes_inherited_pipes_and_reaps_only_owned_child() {
        let fixture = Fixture::new();
        let started = Instant::now();
        let result = output_bounded(
            &mut fixture.command("hang"),
            Duration::from_secs(1),
            None,
            65536,
            65536,
        );
        assert!(matches!(result, Err(OutputError::TimedOut)), "{result:?}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "inherited pipes defeated deadline"
        );
        fixture.assert_owned_child_reaped();
        fixture.assert_descendant_alive();
    }

    #[test]
    fn cancellation_closes_inherited_pipes_and_reaps_only_owned_child() {
        let fixture = Fixture::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let directory = fixture.0.path().to_owned();
        let canceller = thread::spawn(move || {
            wait_file(&directory.join("heartbeat"));
            cancel.store(true, Ordering::Release);
            Instant::now()
        });
        let result = output_bounded(
            &mut fixture.command("hang"),
            Duration::from_secs(3),
            Some(&cancelled),
            65536,
            65536,
        );
        let cancelled_at = canceller.join().unwrap();
        assert!(matches!(result, Err(OutputError::Cancelled)), "{result:?}");
        assert!(
            cancelled_at.elapsed() < Duration::from_secs(1),
            "inherited pipes defeated cancellation"
        );
        fixture.assert_owned_child_reaped();
        fixture.assert_descendant_alive();
    }

    #[test]
    fn parent_exit_drains_available_output_without_waiting_for_descendant() {
        let fixture = Fixture::new();
        let started = Instant::now();
        let output = output_bounded(
            &mut fixture.command("exit"),
            Duration::from_secs(3),
            None,
            65536,
            65536,
        )
        .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains("parent stdout"));
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("parent stderr"));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "parent exit waited for inherited-pipe EOF"
        );
        fixture.assert_owned_child_reaped();
        fixture.assert_descendant_alive();
    }

    #[test]
    fn output_overflow_is_an_error_and_reaps_owned_child() {
        for (mode, stream) in [("stdout-overflow", "stdout"), ("stderr-overflow", "stderr")] {
            let fixture = Fixture::new();
            let result = output_bounded(
                &mut fixture.command(mode),
                Duration::from_secs(3),
                None,
                4096,
                4096,
            );
            assert!(
                matches!(result, Err(OutputError::Limit { stream: actual, bytes: 4096 }) if actual == stream),
                "{result:?}"
            );
            fixture.assert_owned_child_reaped();
        }
    }

    #[test]
    fn collects_both_streams_beyond_pipe_capacity() {
        let fixture = Fixture::new();
        let output = output_bounded(
            &mut fixture.command("bounded"),
            Duration::from_secs(3),
            None,
            128 * 1024,
            128 * 1024,
        )
        .unwrap();
        assert!(output.status.success());
        assert!(output.stdout.ends_with(&vec![b'x'; 96 * 1024]));
        assert_eq!(output.stderr, vec![b'y'; 96 * 1024]);
        fixture.assert_owned_child_reaped();
    }

    #[test]
    fn preexisting_cancellation_does_not_start_command() {
        let fixture = Fixture::new();
        let result = output_bounded(
            &mut fixture.command("hang"),
            Duration::from_secs(3),
            Some(&AtomicBool::new(true)),
            65536,
            65536,
        );
        assert!(matches!(result, Err(OutputError::Cancelled)), "{result:?}");
        assert!(!fixture.0.path().join("parent.pid").exists());
    }
}
