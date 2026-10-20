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
