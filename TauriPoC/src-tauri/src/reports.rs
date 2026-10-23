use crate::desktop::State;
use capture_poc_core::{
    doctor::{self, DoctorReport, DoctorRequest, DoctorSources},
    model::Result,
    setup,
};
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub fn export(state: &State, destination: &Path) -> Result<PathBuf> {
    let _admission = state
        .admission
        .lock()
        .map_err(|_| "Operation admission failed")?;
    if state.operations.busy()? {
        return Err("Finish device setup or saving before exporting readiness facts".into());
    }
    let host = match state.tools() {
        Ok(tools) => doctor::run(&DoctorRequest {
            tools,
            storage_dir: state.paths()?.output_root,
            sources: DoctorSources::LogsAndNetwork,
        }),
        Err(error) => DoctorReport::failure("payloadIntegrity", error),
    };
    let identity = setup::identity(&state.paths()?, state.fixture)?;
    let facts = json!({ "schema_version":1, "format":"android-readiness-report", "created_at":chrono::Utc::now().to_rfc3339(), "host":host, "current_certificate":identity, "setup_checks_this_run":state.operations.facts()?, "limitations":["Host prerequisites do not establish phone readiness.","A browser HTTPS check does not establish every application's certificate trust, app attribution, or upstream internet access.","Phone checks apply only to the named device, CA fingerprint, and expiry time. Certificate installation remains manual."] });
    let parent = destination
        .parent()
        .ok_or("Report destination has no parent directory")?;
    let temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    {
        let mut archive = zip::ZipWriter::new(temporary.reopen().map_err(|e| e.to_string())?);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o600);
        archive
            .start_file("readiness.json", options)
            .map_err(|e| e.to_string())?;
        archive
            .write_all(&serde_json::to_vec_pretty(&facts).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        archive
            .start_file("README.txt", options)
            .map_err(|e| e.to_string())?;
        archive.write_all(b"Mobile Capture Android readiness report\nPublic host diagnostics and setup observations from this app run. No captured HTTP bodies, device logs, private keys, control tokens, or routing journal are included.\n").map_err(|e| e.to_string())?;
        archive
            .finish()
            .map_err(|e| e.to_string())?
            .sync_all()
            .map_err(|e| e.to_string())?;
    }
    capture_poc_core::save::copy_archive(temporary.path(), destination)
}
