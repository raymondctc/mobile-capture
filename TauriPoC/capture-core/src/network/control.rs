use crate::{lease, model::*};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const FRAME_LIMIT: usize = 4 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Bootstrap {
    pub paths: CapturePaths,
    pub tools: ToolSelection,
    pub fixture: bool,
    pub request: StartRequest,
    pub directory: PathBuf,
    pub recovery: bool,
    #[serde(default)]
    pub fixture_drop_initial_reply: bool,
    pub ca: PathBuf,
    pub lock: PathBuf,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Descriptor {
    pub port: u16,
    pub token: String,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Frame {
    pub token: String,
    pub command: Action,
}
#[derive(Serialize, Deserialize)]
pub(crate) enum Action {
    Attach,
    Snapshot,
    Marker(String),
    Stop,
    Recover,
    Export,
}
#[derive(Serialize, Deserialize)]
pub(crate) enum Reply {
    Snapshot(Snapshot),
    Stop(StopOutcome),
    Archive(PathBuf),
    Error(String),
}

pub(super) fn send<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > FRAME_LIMIT {
        return Err("Control response exceeded bounded frame".into());
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| stream.write_all(&bytes))
        .map_err(|e| e.to_string())
}
pub(super) fn receive<T: DeserializeOwned>(stream: &mut TcpStream) -> Result<T> {
    receive_with_timeout(stream, Duration::from_secs(2))
}
fn receive_with_timeout<T: DeserializeOwned>(
    stream: &mut TcpStream,
    timeout: Duration,
) -> Result<T> {
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let mut length = [0; 4];
    stream.read_exact(&mut length).map_err(|e| e.to_string())?;
    let length = u32::from_be_bytes(length) as usize;
    if length > FRAME_LIMIT {
        return Err("Control frame exceeds bound".into());
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid control frame {e}"))
}
pub(crate) struct Client {
    pub directory: PathBuf,
    lease: Option<TcpStream>,
}
impl Client {
    pub fn detached(directory: PathBuf) -> Self {
        Self {
            directory,
            lease: None,
        }
    }
    pub fn attach(directory: PathBuf) -> Result<Self> {
        let descriptor: Descriptor = lease::read(&directory.join("runtime/control.json"))?;
        let mut stream = connect(&descriptor)?;
        send(
            &mut stream,
            &Frame {
                token: descriptor.token,
                command: Action::Attach,
            },
        )?;
        match receive_with_timeout(&mut stream, Duration::from_secs(40))? {
            Reply::Snapshot(_) => Ok(Self {
                directory,
                lease: Some(stream),
            }),
            Reply::Error(e) => Err(e),
            _ => Err("Unexpected guardian attachment response".into()),
        }
    }
    pub fn launch(config: Bootstrap) -> Result<Self> {
        let startup_path = config
            .directory
            .join("runtime")
            .join(format!("startup-{}.json", lease::random_token()?));
        lease::write(&startup_path, &config)?;
        let mut command = Command::new(&config.tools.guardian);
        command
            .arg(&startup_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x01000000 | 0x00000008 | 0x00000200);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot launch independent guardian {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if startup_path.with_extension("ready").exists() {
                reap_later(child);
                return Self::attach(config.directory).map_err(|error| {
                    let failure = lease::read::<String>(&startup_path.with_extension("error"))
                        .unwrap_or_default();
                    format!("Guardian attachment failed. {error}. {failure}")
                });
            }
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Err(lease::read::<String>(&startup_path.with_extension("error"))
                    .unwrap_or_else(|_| "Guardian failed before routing".into()));
            }
            if Instant::now() >= deadline {
                reap_later(child);
                return Err(
                    "Guardian startup handshake timed out; session remains recoverable".into(),
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn request(&self, command: Action) -> Result<Reply> {
        let descriptor: Descriptor = lease::read(&self.directory.join("runtime/control.json"))?;
        let mut stream = connect(&descriptor)?;
        send(
            &mut stream,
            &Frame {
                token: descriptor.token,
                command,
            },
        )?;
        receive_with_timeout(&mut stream, Duration::from_secs(40))
    }
    pub fn has_lease(&self) -> bool {
        self.lease.is_some()
    }
}
fn connect(descriptor: &Descriptor) -> Result<TcpStream> {
    let stream = TcpStream::connect_timeout(
        &SocketAddrV4::new(Ipv4Addr::LOCALHOST, descriptor.port).into(),
        Duration::from_millis(500),
    )
    .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    Ok(stream)
}
pub(crate) fn namespace(paths: &CapturePaths, fixture: bool) -> Result<(PathBuf, PathBuf)> {
    if let Some(root) = &paths.runtime_root {
        if !fixture {
            return Err("Private runtime override is restricted to fixture mode".into());
        }
        return Ok((root.join("owner.lock"), root.join("ca")));
    }
    #[cfg(unix)]
    let lock = PathBuf::from(format!("/tmp/capture-companion-{}/owner.lock", unsafe {
        libc::getuid()
    }));
    #[cfg(windows)]
    let lock = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable")?)
        .join("MobileCapture/owner.lock");
    #[cfg(target_os = "macos")]
    let ca = PathBuf::from(std::env::var_os("HOME").ok_or("HOME unavailable")?)
        .join("Library/Application Support/MobileCapture/TauriPoC/ca");
    #[cfg(all(unix, not(target_os = "macos")))]
    let ca = PathBuf::from(std::env::var_os("HOME").ok_or("HOME unavailable")?)
        .join(".local/share/MobileCapture/TauriPoC/ca");
    #[cfg(windows)]
    let ca = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable")?)
        .join("MobileCapture/TauriPoC/ca");
    Ok((lock, ca))
}
pub(crate) fn prior(lock: &Path) -> Result<Option<PathBuf>> {
    let Some(parent) = lock.parent() else {
        return Err("Invalid routing lease path".into());
    };
    let Some(bytes) = crate::files::read(parent, "owner.lock")? else {
        return Ok(None);
    };
    let text = String::from_utf8(bytes).map_err(|_| "Invalid routing lease locator")?;
    if text.trim().is_empty() {
        Ok(None)
    } else {
        let path = PathBuf::from(text.trim());
        if !path.is_absolute() {
            return Err("Routing lease locator is not absolute".into());
        }
        Ok(Some(path))
    }
}

fn reap_later(mut child: Child) {
    thread::spawn(move || {
        let _ = child.wait();
    });
}
