use serde_json::{json, Value};
use std::{
    env, fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["--version"] {
        println!("Mitmproxy: 12.2.3");
        return;
    }
    let config: Value = serde_json::from_slice(
        &fs::read(env::var_os("CAPTURE_ENGINE_CONFIG").expect("private config required")).unwrap(),
    )
    .unwrap();
    let directory = PathBuf::from(config["directory"].as_str().unwrap());
    let runtime = directory.join("runtime");
    let panic_log = runtime.join("fake-engine.stderr.private");
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text = info.to_string();
        let _ = fs::write(&panic_log, text.chars().take(2048).collect::<String>());
        original_hook(info);
    }));
    let forwarding = config["forwarding"] == true;
    let control = env::current_exe()
        .unwrap()
        .with_file_name("fake-engine-control.json");
    let port = args[args.iter().position(|v| v == "--listen-port").unwrap() + 1]
        .parse::<u16>()
        .unwrap();
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|error| {
        fs::write(runtime.join("fake-engine-startup-error.json"), serde_json::to_vec(&json!({"operation":"bind","port":port,"kind":format!("{:?}",error.kind()),"message":error.to_string()})).unwrap()).unwrap();
        std::process::exit(7);
    });
    listener.set_nonblocking(true).unwrap();
    let writer = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(runtime.join(if forwarding {
            "recovery.writer"
        } else {
            "network.writer"
        }))
        .unwrap();
    writer.try_lock().unwrap();
    let ca = args
        .iter()
        .find_map(|s| s.strip_prefix("confdir="))
        .unwrap();
    fs::create_dir_all(ca).unwrap();
    fs::write(
        PathBuf::from(ca).join("mitmproxy-ca-cert.pem"),
        b"FIXTURE PUBLIC CERTIFICATE",
    )
    .unwrap();
    if !forwarding {
        fs::create_dir_all(directory.join("network")).unwrap();
        for(name,bytes)in[("capture.mitm",b"FIXTURE native\n".as_slice()),("events.jsonl",b""),("capture.har",b"{\"log\":{\"version\":\"1.2\",\"entries\":[]}}"),("incomplete.json",b"{\"flow_ids\":[],\"count\":0,\"missing_bodies\":[],\"omitted_flows\":0,\"errors\":[]}")]{fs::write(directory.join("network").join(name),bytes).unwrap();}
    }
    if !forwarding {
        fs::write(runtime.join("coverage.json"),serde_json::to_vec(&json!({"decoded_http_events":0,"tls_errors":0,"omitted_flows":0,"unavailable_bodies":0,"admission_closed":false})).unwrap()).unwrap();
    }
    fs::write(
        runtime.join("engine-ready.json"),
        serde_json::to_vec(&json!({"nonce":config["nonce"]})).unwrap(),
    )
    .unwrap();
    let closed = Arc::new(AtomicBool::new(false));
    let signal = closed.clone();
    thread::spawn(move || {
        let _ = std::io::stdin().read(&mut [0]);
        signal.store(true, Ordering::Release);
    });
    let started = Instant::now();
    let mut admission_closed = false;
    loop {
        if !forwarding
            && !admission_closed
            && started.elapsed().as_millis()
                >= config["policy"]["duration_ms"]
                    .as_u64()
                    .unwrap_or(3_600_000) as u128
        {
            let mut coverage: Value =
                serde_json::from_slice(&fs::read(runtime.join("coverage.json")).unwrap()).unwrap();
            coverage["admission_closed"] = json!(true);
            fs::write(
                runtime.join("coverage.json"),
                serde_json::to_vec(&coverage).unwrap(),
            )
            .unwrap();
            admission_closed = true;
        }
        let mode: Value = fs::read(&control)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(json!({}));
        if mode["exit"] == true {
            std::process::exit(7);
        }
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 14\r\nConnection: close\r\n\r\nfixture engine",
            );
        }
        if mode["hang_stop"] != true
            && (closed.load(Ordering::Acquire)
                || fs::read(runtime.join("engine-stop.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                    .is_some_and(|v| v["nonce"] == config["nonce"]))
        {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    fs::write(runtime.join("engine-receipt.json"),serde_json::to_vec(&json!({"nonce":config["nonce"],"done":true,"partial":false,"ended_at":"2026-10-20T00:00:00Z"})).unwrap()).unwrap();
    drop(writer);
}
