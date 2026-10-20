use crate::model::Result;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
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
    let mut child = command
        .args(["devices", "-l"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!("Cannot run ADB. Install Android platform-tools and put adb on PATH. {e}")
        })?;
    let output = child.stdout.take().ok_or("ADB stdout unavailable")?;
    let errors = child.stderr.take().ok_or("ADB stderr unavailable")?;
    let read = |mut pipe: Box<dyn Read + Send>| {
        let mut bytes = Vec::new();
        pipe.by_ref()
            .take(65536)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    };
    let out = thread::spawn(move || read(Box::new(output)));
    let err = thread::spawn(move || read(Box::new(errors)));
    let status = match wait_bounded(&mut child, Duration::from_secs(5)) {
        Ok(s) => s,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let bytes = out
        .join()
        .map_err(|_| "ADB reader panicked")?
        .map_err(|e| e.to_string())?;
    let errors = err
        .join()
        .map_err(|_| "ADB error reader panicked")?
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!(
            "ADB discovery failed: {}",
            String::from_utf8_lossy(&errors)
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| "ADB discovery was not UTF-8")?;
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
