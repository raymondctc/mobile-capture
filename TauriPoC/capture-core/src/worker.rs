use crate::{
    model::{now, Receipt, Result, StopReason, WorkerConfig},
    process::quiet,
};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

/// Startup requires one UTF-8 JSON frame ending in LF within 65,536 bytes.
/// EOF before LF starts nothing. After a complete frame, lease EOF may race launch.
pub fn run() -> Result<()> {
    let mut input = std::io::stdin().lock();
    let mut config = String::new();
    input
        .by_ref()
        .take(65536)
        .read_line(&mut config)
        .map_err(|e| e.to_string())?;
    if !config.ends_with('\n') {
        return Ok(());
    }
    let config: WorkerConfig =
        serde_json::from_str(&config).map_err(|e| format!("Invalid startup config: {e}"))?;
    config.limits.validate()?;
    if config.serial.is_empty()
        || config.serial.starts_with('-')
        || config.serial.chars().any(char::is_whitespace)
    {
        return Err("Invalid device identifier".into());
    }
    drop(input);
    let closed = Arc::new(AtomicBool::new(false));
    let signal = closed.clone();
    thread::spawn(move || {
        let mut data = [0; 1];
        let _ = std::io::stdin().read(&mut data);
        signal.store(true, Ordering::Release);
    });
    if closed.load(Ordering::Acquire) {
        return Ok(());
    }
    fs::create_dir_all(config.directory.join("logs")).map_err(|e| e.to_string())?;
    let mut raw = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(config.directory.join("logs/device.log"))
        .map_err(|e| e.to_string())?;
    let mut command = Command::new(&config.adb);
    quiet(&mut command);
    command
        .args([
            "-H",
            "127.0.0.1",
            "-P",
            "5037",
            "-s",
            &config.serial,
            "logcat",
            "-v",
            "epoch",
            "-T",
            "1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot start logcat: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("No collector stdout")?;
    let mut stderr = child.stderr.take().ok_or("No collector stderr")?;
    let (send, receive) = mpsc::sync_channel(8);
    let reader = thread::spawn(move || loop {
        let mut data = vec![0; 8192];
        match stdout.read(&mut data) {
            Ok(0) => break,
            Ok(n) => {
                data.truncate(n);
                if send.send(Ok(data)).is_err() {
                    break;
                }
            }
            Err(e) => {
                let _ = send.send(Err(e.to_string()));
                break;
            }
        }
    });
    let errors = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut data = [0; 8192];
        loop {
            match stderr.read(&mut data) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let end = n.min(8192 - bytes.len());
                    bytes.extend_from_slice(&data[..end]);
                }
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let started = Instant::now();
    let mut bytes = 0u64;
    let mut records = 0u64;
    let mut last = None;
    let mut io_error = None;
    let mut stdout_closed = false;
    let mut admit = |data: &[u8]| -> Result<Option<StopReason>> {
        let mut end = data.len().min((config.limits.bytes - bytes) as usize);
        let mut line_count = 0;
        for (i, byte) in data[..end].iter().enumerate() {
            if *byte == b'\n' {
                line_count += 1;
                if records + line_count == config.limits.records {
                    end = i + 1;
                    break;
                }
            }
        }
        if let Err(error) = raw.write_all(&data[..end]).and_then(|_| raw.flush()) {
            return Err(error.to_string());
        }
        bytes += end as u64;
        records += data[..end].iter().filter(|b| **b == b'\n').count() as u64;
        last = data[..end].last().copied().or(last);
        if bytes >= config.limits.bytes {
            return Ok(Some(StopReason::ByteLimit));
        }
        if records >= config.limits.records {
            return Ok(Some(StopReason::RecordLimit));
        }
        Ok(None)
    };
    let mut reason = loop {
        if closed.load(Ordering::Acquire) {
            break StopReason::LeaseClosed;
        }
        if started.elapsed().as_millis() >= config.limits.duration_ms as u128 {
            break StopReason::DurationLimit;
        }
        match receive.recv_timeout(Duration::from_millis(25)) {
            Ok(Ok(data)) => match admit(&data) {
                Ok(Some(reason)) => break reason,
                Ok(None) => {}
                Err(error) => {
                    io_error = Some(error);
                    break StopReason::ReadError;
                }
            },
            Ok(Err(error)) => {
                io_error = Some(error);
                break StopReason::ReadError;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                stdout_closed = true;
                break StopReason::CollectorExit;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    };
    let exited_before_stop = collector_exited(&mut child)?;
    #[cfg(unix)]
    let stopped_by_worker = unsafe { libc::kill(-(child.id() as i32), libc::SIGTERM) } == 0;
    #[cfg(windows)]
    let stopped_by_worker = !exited_before_stop && child.kill().is_ok();
    let mut forced = cfg!(windows) && stopped_by_worker;
    let mut capped = matches!(
        reason,
        StopReason::ByteLimit | StopReason::RecordLimit | StopReason::ReadError
    );
    let mut drain = |child: &mut Child| -> Result<bool> {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if collector_exited(child)? && stdout_closed && errors.is_finished() {
                return Ok(true);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(false);
            }
            let interval = remaining.min(Duration::from_millis(25));
            if stdout_closed {
                thread::sleep(interval);
                continue;
            }
            match receive.recv_timeout(interval) {
                Ok(Ok(data)) if !capped => match admit(&data) {
                    Ok(Some(limit)) => {
                        reason = limit;
                        capped = true;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        io_error = Some(error);
                        reason = StopReason::ReadError;
                        capped = true;
                    }
                },
                Ok(Err(error)) => {
                    io_error = Some(error);
                    reason = StopReason::ReadError;
                    capped = true;
                }
                Ok(Ok(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => stdout_closed = true,
            }
        }
    };
    if !drain(&mut child)? {
        forced = true;
        #[cfg(unix)]
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        let _ = child.kill();
        if !drain(&mut child)? {
            let _ = child.try_wait();
            return Err(
                "Collector did not exit and close its output before the shutdown deadline".into(),
            );
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    #[cfg(unix)]
    let expected_stop = {
        use std::os::unix::process::ExitStatusExt;
        stopped_by_worker && status.signal() == Some(libc::SIGTERM)
    };
    #[cfg(windows)]
    let expected_stop = stopped_by_worker && status.code() == Some(1);
    if reason == StopReason::LeaseClosed
        && (exited_before_stop || (!status.success() && !expected_stop))
    {
        reason = StopReason::CollectorExit;
    }
    reader.join().map_err(|_| "Collector reader panicked")?;
    let mut stderr = errors
        .join()
        .map_err(|_| "Collector error reader panicked")?;
    if let Some(error) = io_error {
        stderr.push_str(&format!("\n{error}"));
    }
    raw.sync_all().map_err(|e| e.to_string())?;
    let receipt = Receipt {
        reason,
        bytes,
        records,
        incomplete_tail: last.is_some_and(|b| b != b'\n'),
        child_reaped: true,
        forced,
        exit_code: status.code(),
        stderr,
        ended_at: now(),
    };
    let temporary = config.directory.join("receipt.pending");
    fs::write(
        &temporary,
        serde_json::to_vec(&receipt).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(temporary, config.directory.join("receipt.json")).map_err(|e| e.to_string())?;
    Ok(())
}

fn collector_exited(child: &mut Child) -> Result<bool> {
    #[cfg(unix)]
    {
        // The unreaped leader reserves its process group ID through escalation.
        let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(unsafe { info.si_pid() != 0 })
    }
    #[cfg(windows)]
    {
        Ok(child.try_wait().map_err(|e| e.to_string())?.is_some())
    }
}
