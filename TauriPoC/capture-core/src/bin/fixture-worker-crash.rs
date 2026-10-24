use std::io::BufRead;
fn main() {
    let mut configuration = String::new();
    let _ = std::io::stdin().lock().read_line(&mut configuration);
    std::process::exit(1);
}
