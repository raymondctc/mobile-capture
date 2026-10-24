use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use std::{env, fs, process::ExitCode};

fn verify() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 5 {
        return Err("Usage: verify-update ARTIFACT SIGNATURE PUBLIC_KEY VERSION".into());
    }
    let public_key = String::from_utf8(STANDARD.decode(fs::read_to_string(&args[3])?.trim())?)?;
    let signature = String::from_utf8(STANDARD.decode(fs::read_to_string(&args[2])?.trim())?)?;
    let signature = Signature::decode(&signature)?;
    PublicKey::decode(&public_key)?.verify(&fs::read(&args[1])?, &signature, true)?;
    let version = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"));
    if version != Some(args[4].as_str()) {
        return Err("Artifact signed version does not match the release version".into());
    }
    Ok(())
}
fn main() -> ExitCode {
    match verify() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Update verification failed. {error}");
            ExitCode::FAILURE
        }
    }
}
