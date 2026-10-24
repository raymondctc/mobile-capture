fn main() {
    if let Err(error) = capture_poc_core::worker::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
