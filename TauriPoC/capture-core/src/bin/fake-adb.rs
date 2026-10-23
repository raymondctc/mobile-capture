use std::{
    env,
    io::{self, Read, Write},
    net::TcpStream,
    thread,
    time::Duration,
};

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["devices", "-l"] {
        println!("List of devices attached\nfixture-stream\tdevice product:poc model:Fixture_Android device:fixture\nfixture-offline\toffline\nfixture-denied\tunauthorized\nfixture-disconnect\tdevice model:Disconnect_Fixture\nfixture-flood\tdevice model:Limit_Fixture\nfixture-flood-gated\tdevice model:Gated_Limit_Fixture\nfixture-app-uid\tdevice model:App_UID_Fixture\nfixture-app-pid\tdevice model:App_PID_Fixture");
        #[cfg(unix)]
        println!("fixture-stop-exit\tdevice model:Stop_Exit_Fixture\nfixture-stop-signal\tdevice model:Stop_Signal_Fixture\nfixture-stop-clean\tdevice model:Clean_Stop_Fixture");
        return;
    }
    if args.len() >= 7
        && args[..5] == ["-H", "127.0.0.1", "-P", "5037", "-s"]
        && android(&args[5], &args[6..])
    {
        return;
    }
    if args.len() >= 7
        && args[..5] == ["-H", "127.0.0.1", "-P", "5037", "-s"]
        && args[6] != "logcat"
    {
        routing(&args[6..]);
        return;
    }
    let legacy = args.len() == 11 && args[6..] == ["logcat", "-v", "epoch", "-T", "1"];
    let modern = args.len() >= 18
        && args[6..17]
            == [
                "logcat", "-b", "main", "-b", "system", "-b", "crash", "-v", "epoch", "-T", "1",
            ]
        && (args.len() == 18 && args[17] == "*:V"
            || args.len() == 19 && args[17].starts_with("--uid=") && args[18] == "*:V");
    if args.len() < 7 || args[..5] != ["-H", "127.0.0.1", "-P", "5037", "-s"] || !(legacy || modern)
    {
        eprintln!("fixture rejects unexpected ADB arguments: {args:?}");
        std::process::exit(2);
    }
    let serial = &args[5];
    if serial.starts_with("fixture-app-") {
        app_stream(&args);
    }

    #[cfg(unix)]
    if serial.starts_with("fixture-descendant-") {
        exited_leader(serial);
    }
    let mut flood = (serial == "fixture-flood-gated").then(|| {
        let address = std::fs::read_to_string(
            env::current_exe()
                .unwrap()
                .with_file_name("fake-adb-flood-control"),
        )
        .unwrap();
        let stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
    });
    let mut output = io::stdout().lock();
    #[cfg(unix)]
    if matches!(
        serial.as_str(),
        "fixture-stop-exit" | "fixture-stop-signal" | "fixture-stop-clean"
    ) {
        let handler = match serial.as_str() {
            "fixture-stop-exit" => exit_on_stop as *const () as libc::sighandler_t,
            "fixture-stop-signal" => signal_on_stop as *const () as libc::sighandler_t,
            _ => libc::SIG_DFL,
        };
        assert_ne!(
            unsafe { libc::signal(libc::SIGTERM, handler) },
            libc::SIG_ERR
        );
        for count in 0..100 {
            writeln!(output, "1792022401.125 123 456 I Fixture: clean {count}").unwrap();
        }
        output.flush().unwrap();
        loop {
            unsafe { libc::pause() };
        }
    }
    output.write_all(b"1792022401.125  123  456 I Fixture: first raw record\r\n\n1792022401.250 123 456 W Fixture: binary \xff retained\nmalformed record\n").unwrap();
    writeln!(
        output,
        "1792022401.375 123 456 I Fixture: owned_pid={}",
        std::process::id()
    )
    .unwrap();
    output.flush().unwrap();
    if let Some(flood) = &mut flood {
        let mut release = [0];
        flood.read_exact(&mut release).unwrap();
        assert_eq!(release, [1]);
    }
    if serial == "fixture-disconnect" {
        output.write_all(b"incomplete tail").unwrap();
        output.flush().unwrap();
        eprintln!("error: device disconnected");
        std::process::exit(1);
    }
    let mut count = 0;
    loop {
        count += 1;
        writeln!(output, "1792022402.000 123 456 I Fixture: live {count}").unwrap();
        output.flush().unwrap();
        if serial != "fixture-flood" && serial != "fixture-flood-gated" {
            thread::sleep(Duration::from_millis(150));
        }
    }
}

#[cfg(unix)]
fn exited_leader(serial: &str) -> ! {
    use std::{io::Read, os::unix::net::UnixStream};

    let mut control = UnixStream::connect(env::var_os("FIXTURE_GROUP_CONTROL").unwrap()).unwrap();
    control
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut pipe = [0; 2];
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    let leader = std::process::id();
    let descendant = unsafe { libc::fork() };
    assert!(descendant >= 0);
    if descendant == 0 {
        if serial.contains("ignore-term") {
            assert_ne!(
                unsafe { libc::signal(libc::SIGTERM, libc::SIG_IGN) },
                libc::SIG_ERR
            );
        }
        unsafe {
            libc::close(pipe[1]);
            assert_eq!(libc::read(pipe[0], [0u8; 1].as_mut_ptr().cast(), 1), 0);
            libc::close(pipe[0]);
        }
        while unsafe { libc::getppid() } == leader as i32 {
            thread::yield_now();
        }
        println!("1792022401.125 123 456 I Fixture: inherited pipes");
        io::stdout().flush().unwrap();
        writeln!(control, "{leader} {}", std::process::id()).unwrap();
        if serial.contains("stderr-only") {
            control.read_exact(&mut [0]).unwrap();
            unsafe { libc::close(libc::STDOUT_FILENO) };
            eprintln!("fixture descendant retains stderr");
            if serial.contains("stderr-cap") {
                io::stderr()
                    .write_all(&vec![
                        b'x';
                        8192 - b"fixture descendant retains stderr\n".len()
                    ])
                    .unwrap();
                io::stderr().flush().unwrap();
            }
            writeln!(control, "stdout closed").unwrap();
        }
        let _ = control.read(&mut [0]);
        unsafe { libc::_exit(0) };
    }
    unsafe {
        libc::close(pipe[0]);
        match serial.rsplit('-').next().unwrap() {
            "error" => libc::_exit(23),
            "signal" => {
                libc::kill(libc::getpid(), libc::SIGUSR1);
                libc::_exit(24);
            }
            _ => libc::_exit(0),
        }
    }
}

#[cfg(unix)]
extern "C" fn exit_on_stop(_: libc::c_int) {
    unsafe { libc::_exit(23) };
}

#[cfg(unix)]
extern "C" fn signal_on_stop(_: libc::c_int) {
    unsafe {
        libc::kill(libc::getpid(), libc::SIGUSR1);
        libc::_exit(24);
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FakeState {
    #[serde(default)]
    settings: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    reverse: std::collections::BTreeMap<String, String>,
    #[serde(default = "yes")]
    connected: bool,
    #[serde(default = "yes")]
    acknowledge_clear: bool,
    #[serde(default)]
    commands: Vec<Vec<String>>,
    #[serde(default)]
    fail_command: Option<usize>,
    #[serde(default)]
    pushed_certificates: std::collections::BTreeMap<String, String>,
}
fn yes() -> bool {
    true
}
fn routing(args: &[String]) {
    let path = env::var_os("FAKE_ADB_STATE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            env::current_exe()
                .unwrap()
                .with_file_name("fake-adb-state.json")
        });
    let mut state: FakeState =
        serde_json::from_slice(&std::fs::read(&path).expect("private fake state required"))
            .unwrap();
    state.commands.push(args.to_vec());
    let failed = !state.connected || state.fail_command == Some(state.commands.len());
    let mut output = String::new();
    let mut conflict = false;
    if !failed {
        if args == ["shell", "settings list global"] {
            output = state
                .settings
                .iter()
                .map(|(k, v)| format!("{k}={v}\n"))
                .collect();
        } else if args == ["reverse", "--list"] {
            output = state
                .reverse
                .iter()
                .map(|(k, v)| format!("fixture {k} {v}\n"))
                .collect();
        } else if args.len() == 4 && args[..2] == ["reverse", "--no-rebind"] {
            if state.reverse.contains_key(&args[2]) {
                conflict = true;
            } else {
                state.reverse.insert(args[2].clone(), args[3].clone());
            }
        } else if args.len() == 3 && args[..2] == ["reverse", "--remove"] {
            state.reverse.remove(&args[2]);
        } else if args.len() == 3 && args[0] == "push" {
            let suffix = args[2]
                .strip_prefix("/sdcard/Download/mobile-capture-ca-")
                .and_then(|s| s.strip_suffix(".crt"))
                .expect("fixture accepts only the public setup certificate destination");
            assert!(suffix.len() == 12 && suffix.bytes().all(|b| b.is_ascii_hexdigit()));
            let pem = std::fs::read_to_string(&args[1]).unwrap();
            let middle = pem
                .trim()
                .strip_prefix("-----BEGIN CERTIFICATE-----")
                .and_then(|s| s.strip_suffix("-----END CERTIFICATE-----"))
                .expect("fixture accepts only public PEM certificate staging");
            assert!(pem.len() <= 65536 && !pem.contains("PRIVATE") && !middle.contains("-----"));
            state.pushed_certificates.insert(args[2].clone(), pem);
        } else if args.len() == 7
            && args[..6]
                == [
                    "shell",
                    "am",
                    "start",
                    "-a",
                    "android.intent.action.VIEW",
                    "-d",
                ]
        {
            let nonce = args[6]
                .strip_prefix("https://mitm.it/mobile-capture-check/")
                .and_then(|s| s.strip_suffix("/https"))
                .expect("fixture accepts only an owned HTTPS check intent");
            assert!(nonce.len() == 32 && nonce.bytes().all(|b| b.is_ascii_hexdigit()));
        } else if args.len() == 2 && args[0] == "shell" {
            let fields = shell_fields(&args[1]);
            assert!(fields.len() >= 4 && fields[0] == "settings" && fields[2] == "global");
            if fields[1] == "delete" {
                state.settings.remove(&fields[3]);
            } else {
                state.settings.insert(fields[3].clone(), fields[4].clone());
                if fields[3] == "http_proxy" && fields[4] == ":0" && state.acknowledge_clear {
                    for (k, v) in [
                        ("global_http_proxy_host", ""),
                        ("global_http_proxy_port", "0"),
                        ("global_http_proxy_exclusion_list", ""),
                        ("global_proxy_pac_url", ""),
                    ] {
                        state.settings.insert(k.into(), v.into());
                    }
                }
            }
        } else {
            panic!("fixture rejects arguments {args:?}");
        }
    }
    let temporary = path.with_extension("pending");
    std::fs::write(&temporary, serde_json::to_vec(&state).unwrap()).unwrap();
    std::fs::rename(temporary, path).unwrap();
    if failed || conflict {
        eprintln!("fixture device disconnected or reverse conflict");
        std::process::exit(1);
    }
    print!("{output}");
}
fn shell_fields(script: &str) -> Vec<String> {
    let mut fields = vec![];
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut active = false;
    for c in script.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            active = true;
        } else if c == '\\' && !quoted {
            escaped = true;
        } else if c == '\'' {
            quoted = !quoted;
            active = true;
        } else if c == ' ' && !quoted {
            if active {
                fields.push(std::mem::take(&mut current));
                active = false;
            }
        } else {
            current.push(c);
            active = true;
        }
    }
    if active {
        fields.push(current);
    }
    assert!(!quoted && !escaped);
    fields
}

fn android_state() -> serde_json::Value {
    std::fs::read(env::current_exe().unwrap().with_file_name("fake-android-state.json")).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_else(||serde_json::json!({"user":0,"uid":10001,"install":"v1","shared":false,"uid_supported":true,"processes":true,"clock_offset":0.0}))
}
fn android(serial: &str, args: &[String]) -> bool {
    let state = android_state();
    let user = state["user"].as_u64().unwrap_or(0);
    let uid = state["uid"].as_u64().unwrap_or(10001);
    let packages = format!(
        "package:com.fixture.app uid:{uid}\npackage:com.fixture.other uid:{}\n",
        if state["shared"] == true {
            uid
        } else {
            uid + 1
        }
    );
    let install = format!(
        "codePath=/data/app/fixture-{}\nfirstInstallTime=2026-01-01\nlastUpdateTime={}\n",
        state["install"].as_str().unwrap_or("v1"),
        state["install"].as_str().unwrap_or("v1")
    );
    if args == ["shell", "am", "get-current-user"] {
        println!("{user}");
    } else if args.len() == 7 && args[..4] == ["shell", "pm", "list", "packages"] {
        print!("{packages}");
    } else if args.len() == 4 && args[..3] == ["shell", "dumpsys", "package"] {
        print!("{install}");
    } else if args == ["shell", "logcat", "--help"] {
        if state["uid_supported"] != false && serial != "fixture-app-pid" {
            println!("--uid=<uid> only show matching UID");
        }
    } else if args.len() == 7
        && args[0] == "logcat"
        && args[1].starts_with("--uid=")
        && args[2..] == ["-d", "-t", "1", "-v", "epoch"]
    {
        if state["uid_supported"] == false || serial == "fixture-app-pid" {
            std::process::exit(1);
        }
    } else if args.len() == 2 && args[0] == "shell" && args[1].contains("__MC_FENCE__") {
        if let Some(delay) = state["probe_delay_ms"].as_u64() {
            std::fs::write(
                env::current_exe()
                    .unwrap()
                    .with_file_name("fake-android-probe-started"),
                b"ready",
            )
            .unwrap();
            thread::sleep(Duration::from_millis(delay));
        }
        let epoch = chrono::Utc::now().timestamp_micros() as f64 / 1_000_000.0
            + state["clock_offset"].as_f64().unwrap_or(0.0);
        println!("__MC_FENCE__\n{epoch:.6}\n__MC_USER__\n{user}\n__MC_PACKAGES__\n{packages}__MC_PACKAGE__\n{install}");
        if args[1].contains("__MC_CLOCK__") {
            println!(
                "__MC_CLOCK__\n{epoch:.6}\n100.0 100.0\n{epoch:.6}\n100\n__MC_PS__\nPID UID NAME"
            );
            if state["processes"] != false {
                println!("123 {uid} com.fixture.custom");
            }
            println!("__MC_DETAILS__");
            if state["processes"] != false {
                let mut fields = vec!["0"; 20];
                fields[0] = "S";
                fields[19] = "500";
                println!("__MC_PID_123\n123 (fixture worker) {}\nUid: {uid} {uid} {uid} {uid}\ncom.fixture.custom", fields.join(" "));
            }
        }
        println!("__MC_END__");
    } else {
        return false;
    }
    true
}
fn app_stream(args: &[String]) -> ! {
    let uid_filtered = args.iter().any(|a| a.starts_with("--uid="));
    let mut output = io::stdout().lock();
    let mut count = 0;
    loop {
        let epoch = chrono::Utc::now().timestamp_micros() as f64 / 1_000_000.0;
        if count == 0 {
            writeln!(output, "orphan private continuation").unwrap();
        }
        if !uid_filtered {
            writeln!(
                output,
                "{epoch:.6} 999 999 I Other: private-other\nprivate-other-continuation"
            )
            .unwrap();
        }
        writeln!(
            output,
            "{epoch:.6} 123 456 V App: owned-{count}\nowned-stack-{count}"
        )
        .unwrap();
        output.flush().unwrap();
        count += 1;
        thread::sleep(Duration::from_millis(50));
    }
}
