fn main() {
    let path = std::env::args_os().nth(1).expect("private startup config");
    if let Err(error) = capture_poc_core::run_guardian(std::path::Path::new(&path)) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
