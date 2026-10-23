use capture_poc_core::{
    doctor::{self, DoctorReport, DoctorRequest, DoctorSources},
    toolchain,
};
use std::{collections::BTreeMap, path::PathBuf};

fn arguments(args: Vec<String>) -> Result<DoctorRequest, (i32, &'static str, String)> {
    let fail = |message: String| (64, "invalidArguments", message);
    let mut values = BTreeMap::new();
    let mut json = false;
    let mut iter = args.into_iter();
    while let Some(key) = iter.next() {
        if key == "--json" {
            if json {
                return Err(fail("Duplicate --json".into()));
            }
            json = true;
            continue;
        }
        if !["--resource-dir", "--storage-dir", "--sources"].contains(&key.as_str()) {
            return Err(fail(format!("Unknown argument: {key}")));
        }
        let value = iter
            .next()
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| fail(format!("Missing value for {key}")))?;
        if values.insert(key.clone(), value).is_some() {
            return Err(fail(format!("Duplicate {key}")));
        }
    }
    if !json {
        return Err(fail("--json is required".into()));
    }
    let sources = match values
        .get("--sources")
        .map(String::as_str)
        .unwrap_or("logs,network")
    {
        "logs" => DoctorSources::Logs,
        "network" => DoctorSources::Network,
        "logs,network" | "network,logs" => DoctorSources::LogsAndNetwork,
        _ => {
            return Err(fail(
                "--sources must be logs, network, or logs,network".into(),
            ))
        }
    };
    let storage_dir = values
        .get("--storage-dir")
        .map(PathBuf::from)
        .ok_or_else(|| fail("--storage-dir is required".into()))?;
    if !storage_dir.is_absolute() {
        return Err(fail("--storage-dir must be absolute".into()));
    }
    let resource_dir = if let Some(value) = values.get("--resource-dir") {
        PathBuf::from(value)
    } else {
        let exe = std::env::current_exe().map_err(|e| (70, "owningApplication", e.to_string()))?;
        let directory = exe
            .parent()
            .ok_or_else(|| (70, "owningApplication", "Executable has no parent".into()))?;
        if cfg!(target_os = "macos") && directory.file_name().is_some_and(|name| name == "MacOS") {
            directory.parent().unwrap().join("Resources")
        } else {
            directory.into()
        }
    };
    if !resource_dir.is_absolute() {
        return Err(fail("--resource-dir must be absolute".into()));
    }
    let tools =
        toolchain::resolve_bundled(&resource_dir).map_err(|e| (10, "payloadIntegrity", e))?;
    Ok(DoctorRequest {
        tools,
        storage_dir,
        sources,
    })
}
fn main() {
    let (status, report) = match arguments(std::env::args().skip(1).collect()) {
        Ok(request) => {
            let report = doctor::run(&request);
            (report.exit_code(), report)
        }
        Err((status, code, message)) => (status, DoctorReport::failure(code, message)),
    };
    match serde_json::to_string(&report) {
        Ok(json) => println!("{json}"),
        Err(_) => {
            println!("{{\"schemaVersion\":1,\"command\":\"doctor\",\"error\":{{\"code\":\"serialization\",\"message\":\"Cannot serialize Doctor report\"}}}}");
            std::process::exit(70);
        }
    }
    std::process::exit(status);
}
