use crate::{
    model::Result,
    process,
    toolchain::{self, ToolSource, Toolchain},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DoctorSources {
    Logs,
    Network,
    #[default]
    LogsAndNetwork,
}

#[derive(Clone, Debug)]
pub struct DoctorRequest {
    pub tools: Toolchain,
    pub storage_dir: PathBuf,
    pub sources: DoctorSources,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CheckStatus {
    Ready,
    NeedsAction,
    Missing,
    Incompatible,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub id: String,
    pub status: CheckStatus,
    pub message: String,
    pub source: String,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSetup {
    pub status: String,
    pub manual_actions: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorError {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub schema_version: u32,
    pub command: String,
    pub scope: String,
    pub platform: String,
    pub checked_at: String,
    pub host_target: String,
    pub host_prerequisites_ready: bool,
    pub sources: DoctorSources,
    pub checks: Vec<DoctorCheck>,
    pub device_setup: DeviceSetup,
    pub error: Option<DoctorError>,
}
impl DoctorReport {
    pub fn exit_code(&self) -> i32 {
        if let Some(error) = &self.error {
            match error.code.as_str() {
                "invalidArguments" => 64,
                "payloadIntegrity" => 10,
                _ => 70,
            }
        } else if self.host_prerequisites_ready {
            0
        } else {
            10
        }
    }
    pub fn failure(code: &str, message: String) -> Self {
        let mut report = Self::empty(DoctorSources::default());
        report.error = Some(DoctorError {
            code: code.into(),
            message,
        });
        report
    }
    fn empty(sources: DoctorSources) -> Self {
        Self { schema_version: 1, command: "doctor".into(), scope: "hostPrerequisites".into(), platform: "android".into(), checked_at: chrono::Utc::now().to_rfc3339(), host_target: toolchain::host_target().into(), host_prerequisites_ready: false, sources, checks: Vec::new(), device_setup: DeviceSetup { status: "notChecked".into(), manual_actions: vec!["Enable USB debugging and approve this computer on the selected Android device.".into(), "For HTTPS capture, install the exported public CA certificate in Android Settings and confirm the selected app trusts user certificates.".into(), "Start an explicit connection check or capture to verify phone traffic. Host readiness does not establish device readiness.".into()] }, error: None }
    }
}

pub fn run(request: &DoctorRequest) -> DoctorReport {
    let mut report = DoctorReport::empty(request.sources);
    let source = match request.tools.source {
        ToolSource::Bundled => "bundled",
        ToolSource::External => "external",
        ToolSource::Fixture => "fixture",
    };
    for (id, path) in [
        ("worker", &request.tools.worker),
        ("guardian", &request.tools.guardian),
    ] {
        report.checks.push(check(
            id,
            path,
            source,
            toolchain::verify_architecture(path, &request.tools.target),
            None,
        ));
    }
    report.checks.push(check(
        "addon",
        &request.tools.addon,
        source,
        addon(&request.tools.addon),
        None,
    ));
    report.checks.push(DoctorCheck { id: "payloadIntegrity".into(), status: if request.tools.source == ToolSource::Bundled { CheckStatus::Ready } else { CheckStatus::NeedsAction }, message: if request.tools.source == ToolSource::Bundled { "All packaged components matched the compiled payload manifest." } else { "Explicit development tools are selected. A complete installed payload is required for distribution." }.into(), source: source.into(), path: None, version: None });
    let adb = probe(&request.tools.adb, &["version"]);
    let adb_version = adb.as_ref().ok().and_then(|text| {
        text.lines()
            .find(|line| line.starts_with("Version "))
            .map(str::to_owned)
    });
    let result = adb.and_then(|text| {
        if text.contains("Android Debug Bridge version 1.0.41") && text.contains("37.0.1") {
            probe(&request.tools.adb, &["help"]).and_then(|help| {
                if ["logcat", "reverse", "shell", "push"]
                    .iter()
                    .all(|word| help.contains(word))
                {
                    Ok(())
                } else {
                    Err("ADB does not expose required capture commands".into())
                }
            })
        } else {
            Err("ADB 37.0.1 is required".into())
        }
    });
    report.checks.push(check(
        "adb",
        &request.tools.adb,
        source,
        result,
        adb_version,
    ));
    if request.sources != DoctorSources::Logs {
        let version = probe(&request.tools.mitmdump, &["--version"]);
        let display = version.as_ref().ok().and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("Mitmproxy:"))
                .map(str::to_owned)
        });
        let result = version.and_then(|text| {
            if text.lines().any(|line| {
                ["Mitmproxy: 12.2.3", "Mitmproxy: 12.2.3 binary"].contains(&line.trim())
            }) {
                probe(&request.tools.mitmdump, &["--options"]).and_then(|text| {
                    if ["listen_host:", "listen_port:", "confdir:", "ssl_insecure:"]
                        .iter()
                        .all(|word| text.contains(word))
                    {
                        Ok(())
                    } else {
                        Err("mitmdump does not expose required capture options".into())
                    }
                })
            } else {
                Err("mitmdump 12.2.3 is required".into())
            }
        });
        report.checks.push(check(
            "mitmdump",
            &request.tools.mitmdump,
            source,
            result,
            display,
        ));
    }
    report.checks.push(check(
        "storage",
        &request.storage_dir,
        "system",
        storage(&request.storage_dir),
        None,
    ));
    report.host_prerequisites_ready = report
        .checks
        .iter()
        .all(|check| check.status == CheckStatus::Ready);
    report
}

fn check(
    id: &str,
    path: &Path,
    source: &str,
    result: Result<()>,
    version: Option<String>,
) -> DoctorCheck {
    let (status, message) = match result {
        Ok(()) => (CheckStatus::Ready, "Local prerequisite is ready.".into()),
        Err(error) => (
            if path.exists() {
                CheckStatus::Incompatible
            } else {
                CheckStatus::Missing
            },
            error,
        ),
    };
    DoctorCheck {
        id: id.into(),
        status,
        message,
        source: source.into(),
        path: Some(path.into()),
        version,
    }
}
fn addon(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("Network addon is not a bounded file".into());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if !text.contains("mitmproxy") || !text.contains("addons") {
        return Err("Network addon contract is missing".into());
    }
    Ok(())
}
fn probe(path: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new(path);
    process::quiet(&mut command);
    command
        .args(args)
        .env("PATH", "")
        .env("PYTHONNOUSERSITE", "1")
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME");
    let output = process::output_bounded(
        &mut command,
        Duration::from_secs(30),
        None,
        256 * 1024,
        256 * 1024,
    )
    .map_err(|error| format!("Local tool probe failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Local tool probe failed: {}",
            String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(1024)])
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "Local tool probe was not UTF-8".into())
}

fn storage(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err("Storage must be an absolute existing private directory".into());
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Storage must be a real private directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(
                "Storage must belong to the current user and deny group/other access".into(),
            );
        }
    }
    #[cfg(windows)]
    windows_private(path)
        .map_err(|e| format!("Storage privacy check failed for {}: {e}", path.display()))?;
    let free = available_space(path)
        .map_err(|e| format!("Check available storage space for {}: {e}", path.display()))?;
    if free < 512 * 1024 * 1024 {
        return Err(format!(
            "Storage has {free} bytes free. At least 512 MiB is required."
        ));
    }
    let mut probe = tempfile::NamedTempFile::new_in(path)
        .map_err(|e| format!("Storage is not writable: {e}"))?;
    probe
        .write_all(b"mobile-capture-doctor\n")
        .and_then(|_| probe.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(unix)]
fn available_space(path: &Path) -> Result<u64> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let name = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(name.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let stat = unsafe { stat.assume_init() };
    Ok((stat.f_bavail as u64).saturating_mul(stat.f_frsize))
}
#[cfg(windows)]
fn available_space(path: &Path) -> Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0;
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            name.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(free)
}
#[cfg(windows)]
fn windows_private(path: &Path) -> Result<()> {
    use std::{
        mem::size_of,
        os::windows::{ffi::OsStrExt, fs::MetadataExt},
    };
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::*,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    if fs::metadata(path)
        .map_err(|e| e.to_string())?
        .file_attributes()
        & 0x400
        != 0
    {
        return Err("Storage reparse point refused".into());
    }
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let result = (|| {
        let mut length = 0;
        unsafe {
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length);
        }
        if length == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut user = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                user.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let user_sid = unsafe { (*(user.as_ptr() as *const TOKEN_USER)).User.Sid };
        let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let information = DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION;
        length = 0;
        unsafe {
            GetFileSecurityW(
                name.as_ptr(),
                information,
                std::ptr::null_mut(),
                0,
                &mut length,
            );
        }
        if length == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut bytes = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        let descriptor = bytes.as_mut_ptr().cast();
        if unsafe { GetFileSecurityW(name.as_ptr(), information, descriptor, length, &mut length) }
            == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut defaulted = 0;
        let mut owner = std::ptr::null_mut();
        if unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) } == 0 {
            return Err(format!(
                "Read storage owner: {}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { EqualSid(owner, user_sid) } == 0 {
            return Err("Storage belongs to another user".into());
        }
        let mut present = 0;
        let mut acl = std::ptr::null_mut();
        if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) }
            == 0
            || present == 0
            || acl.is_null()
        {
            return Err("Storage has no private access control list".into());
        }
        let mut system = [0u32; 17];
        let mut administrators = [0u32; 17];
        for (kind, sid) in [
            (WinLocalSystemSid, system.as_mut_ptr()),
            (WinBuiltinAdministratorsSid, administrators.as_mut_ptr()),
        ] {
            let mut size = 68;
            if unsafe { CreateWellKnownSid(kind, std::ptr::null_mut(), sid.cast(), &mut size) } == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        for i in 0..unsafe { (*acl).AceCount } {
            let mut entry = std::ptr::null_mut();
            if unsafe { GetAce(acl, i as u32, &mut entry) } == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let header = unsafe { &*(entry as *const ACE_HEADER) };
            if header.AceFlags & INHERIT_ONLY_ACE as u8 != 0 {
                continue;
            }
            if header.AceType == 1 {
                continue;
            }
            if header.AceType != 0 {
                return Err("Storage uses an unsupported access control entry".into());
            }
            let ace = unsafe { &*(entry as *const ACCESS_ALLOWED_ACE) };
            let sid = (&ace.SidStart as *const u32).cast_mut().cast();
            if ace.Mask != 0
                && ![
                    user_sid,
                    system.as_mut_ptr().cast(),
                    administrators.as_mut_ptr().cast(),
                ]
                .iter()
                .any(|allowed| unsafe { EqualSid(sid, *allowed) } != 0)
            {
                return Err(
                    "Storage grants access to another user. Select a private directory.".into(),
                );
            }
        }
        Ok(())
    })();
    unsafe {
        CloseHandle(token);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_keeps_manual_device_readiness_separate() {
        let report = DoctorReport::failure("internal", "missing".into());
        assert_eq!(report.exit_code(), 70);
        assert!(!report.host_prerequisites_ready);
        assert_eq!(report.device_setup.status, "notChecked");
        assert!(!report.device_setup.manual_actions.is_empty());
    }
    #[test]
    fn rejects_relative_storage_without_creating_it() {
        assert!(storage(Path::new("relative-doctor-output")).is_err());
        assert!(!Path::new("relative-doctor-output").exists());
    }
    #[test]
    fn private_writable_storage_passes() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        #[cfg(windows)]
        crate::lease::private_directory(dir.path()).unwrap();
        storage(dir.path()).expect("current-user private storage should pass admission");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[cfg(unix)]
    #[test]
    fn probes_versions_and_capabilities_without_contacting_a_device() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let log = dir.path().join("commands.txt");
        let adb = dir.path().join("adb");
        let mitmdump = dir.path().join("mitmdump");
        let script = |path: &Path, cases: &str| {
            fs::write(path, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n{}\n*) exit 99;;\nesac\n", log.display(), cases)).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        };
        script(&adb, "version) printf 'Android Debug Bridge version 1.0.41\\nVersion 37.0.1-15733141\\n';;\nhelp) printf 'logcat reverse shell push\\n';;");
        script(&mitmdump, "--version) printf 'Mitmproxy: 12.2.3 binary\\n';;\n--options) printf 'listen_host: localhost\\nlisten_port: 8080\\nconfdir: .\\nssl_insecure: false\\n';;");
        let worker = dir.path().join("worker");
        fs::write(&worker, [0xcf, 0xfa, 0xed, 0xfe, 12, 0, 0, 1]).unwrap();
        let addon = dir.path().join("addon.py");
        fs::write(&addon, "from mitmproxy import http\naddons = []\n").unwrap();
        let report = run(&DoctorRequest {
            tools: Toolchain {
                adb,
                mitmdump,
                worker: worker.clone(),
                guardian: worker,
                addon,
                source: ToolSource::External,
                target: "aarch64-apple-darwin".into(),
            },
            storage_dir: dir.path().into(),
            sources: DoctorSources::LogsAndNetwork,
        });
        assert!(report
            .checks
            .iter()
            .filter(|check| ["adb", "mitmdump"].contains(&check.id.as_str()))
            .all(|check| check.status == CheckStatus::Ready));
        assert_eq!(
            fs::read_to_string(log).unwrap().lines().collect::<Vec<_>>(),
            ["version", "help", "--version", "--options"]
        );
    }
}
