use std::{
    env,
    io::{self, Write},
    thread,
    time::Duration,
};

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["devices", "-l"] {
        println!("List of devices attached\nfixture-stream\tdevice product:poc model:Fixture_Android device:fixture\nfixture-offline\toffline\nfixture-denied\tunauthorized\nfixture-disconnect\tdevice model:Disconnect_Fixture\nfixture-flood\tdevice model:Limit_Fixture");
        #[cfg(unix)]
        println!("fixture-stop-exit\tdevice model:Stop_Exit_Fixture\nfixture-stop-signal\tdevice model:Stop_Signal_Fixture\nfixture-stop-clean\tdevice model:Clean_Stop_Fixture");
        return;
    }
    if args.len() != 11
        || args[..5] != ["-H", "127.0.0.1", "-P", "5037", "-s"]
        || args[6..] != ["logcat", "-v", "epoch", "-T", "1"]
    {
        eprintln!("fixture rejects unexpected ADB arguments: {args:?}");
        std::process::exit(2);
    }
    let serial = &args[5];
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
        if serial != "fixture-flood" {
            thread::sleep(Duration::from_millis(150));
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
