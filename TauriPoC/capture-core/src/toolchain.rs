use crate::model::{Result, ToolSelection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolSource {
    Bundled,
    External,
    Fixture,
}

#[derive(Clone, Debug)]
pub struct Toolchain {
    pub adb: PathBuf,
    pub mitmdump: PathBuf,
    pub worker: PathBuf,
    pub guardian: PathBuf,
    pub addon: PathBuf,
    pub source: ToolSource,
    pub target: String,
}

impl Toolchain {
    pub fn selection(&self) -> ToolSelection {
        ToolSelection {
            adb: self.adb.clone(),
            mitmdump: Some(self.mitmdump.clone()),
            worker: self.worker.clone(),
            guardian: self.guardian.clone(),
            addon: self.addon.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    target: String,
    adb_version: String,
    mitmproxy_version: String,
    tools: Tools,
    files: Vec<PayloadFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tools {
    adb: String,
    mitmdump: String,
    worker: String,
    guardian: String,
    addon: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayloadFile {
    path: String,
    kind: String,
    bytes: u64,
    sha256: String,
    target: Option<String>,
}

pub fn host_target() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else {
        "unsupported"
    }
}

pub fn resolve_bundled(resource_dir: &Path) -> Result<Toolchain> {
    let digest = option_env!("MOBILE_CAPTURE_PAYLOAD_SHA256")
        .ok_or("This build has no pinned Android payload. Use the packaging command or explicit development tools.")?;
    resolve_verified(resource_dir, digest, host_target())
}

fn relative(path: &str) -> Result<&Path> {
    let value = Path::new(path);
    if path.is_empty()
        || path.contains('\\')
        || path.contains(':')
        || value
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("Unsafe payload path: {path}"));
    }
    Ok(value)
}

fn resolve_verified(resource_dir: &Path, digest: &str, target: &str) -> Result<Toolchain> {
    let root = resource_dir.join("android-tools");
    let metadata = fs::symlink_metadata(&root)
        .map_err(|e| format!("Bundled Android payload is missing: {e}"))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Bundled payload root reparse point refused".into());
        }
    }
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Bundled payload root is not a real directory".into());
    }
    let manifest_path = root.join("payload.json");
    let metadata = fs::symlink_metadata(&manifest_path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4 * 1024 * 1024
    {
        return Err("Invalid bundled payload manifest".into());
    }
    let bytes = fs::read(manifest_path).map_err(|e| e.to_string())?;
    if format!("{:x}", Sha256::digest(&bytes)) != digest {
        return Err("Bundled payload manifest integrity failed".into());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Invalid bundled payload manifest: {e}"))?;
    if manifest.schema_version != 1
        || manifest.target != target
        || target == "unsupported"
        || manifest.adb_version != "37.0.1"
        || manifest.mitmproxy_version != "12.2.3"
    {
        return Err("Bundled payload version or host architecture is incompatible".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut declared = BTreeSet::new();
    for file in &manifest.files {
        let path = root.join(relative(&file.path)?);
        if !declared.insert(file.path.as_str()) {
            return Err("Duplicate bundled payload entry".into());
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| format!("Missing bundled component {}: {e}", file.path))?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Bundled reparse point refused".into());
            }
        }
        let canonical = path.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(&root) {
            return Err("Bundled component escapes its payload".into());
        }
        let (size, hash) = if file.kind == "symlink" && metadata.file_type().is_symlink() {
            let actual = fs::read_link(&path).map_err(|e| e.to_string())?;
            if actual.is_absolute() || actual.to_str() != file.target.as_deref() {
                return Err("Bundled symlink target changed".into());
            }
            let value = actual
                .to_str()
                .ok_or("Non-UTF8 bundled symlink")?
                .as_bytes();
            (value.len() as u64, format!("{:x}", Sha256::digest(value)))
        } else if file.kind == "file" && metadata.is_file() && !metadata.file_type().is_symlink() {
            let mut handle = fs::File::open(&path).map_err(|e| e.to_string())?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                let count = handle.read(&mut buffer).map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            (metadata.len(), format!("{:x}", hash.finalize()))
        } else {
            return Err(format!("Invalid bundled component type: {}", file.path));
        };
        if size != file.bytes || hash != file.sha256 {
            return Err(format!("Bundled component integrity failed: {}", file.path));
        }
    }
    fn observed(root: &Path, directory: &Path, paths: &mut BTreeSet<String>) -> Result<()> {
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if fs::symlink_metadata(&path)
                    .map_err(|e| e.to_string())?
                    .file_attributes()
                    & 0x400
                    != 0
                {
                    return Err("Bundled directory reparse point refused".into());
                }
            }
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                observed(root, &path, paths)?;
            } else {
                paths.insert(
                    path.strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_str()
                        .ok_or("Non-UTF8 payload path")?
                        .replace('\\', "/"),
                );
            }
        }
        Ok(())
    }
    let mut actual = BTreeSet::new();
    observed(&root, &root, &mut actual)?;
    actual.remove("payload.json");
    if actual.iter().map(String::as_str).collect::<BTreeSet<_>>() != declared {
        return Err("Bundled payload contains undeclared or missing files".into());
    }
    let tool = |value: &str| -> Result<PathBuf> {
        if !declared.contains(value) {
            return Err(format!("Undeclared bundled tool: {value}"));
        }
        let path = root.join(relative(value)?);
        if !path.is_file() {
            return Err("Bundled tool is not a file".into());
        }
        Ok(path)
    };
    let result = Toolchain {
        adb: tool(&manifest.tools.adb)?,
        mitmdump: tool(&manifest.tools.mitmdump)?,
        worker: tool(&manifest.tools.worker)?,
        guardian: tool(&manifest.tools.guardian)?,
        addon: tool(&manifest.tools.addon)?,
        source: ToolSource::Bundled,
        target: manifest.target,
    };
    for path in [
        &result.adb,
        &result.mitmdump,
        &result.worker,
        &result.guardian,
    ] {
        verify_architecture(path, target)?;
    }
    Ok(result)
}

pub fn verify_architecture(path: &Path, target: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err(format!(
                "Bundled tool is not executable: {}",
                path.display()
            ));
        }
    }
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = [0u8; 4096];
    let count = file.read(&mut bytes).map_err(|e| e.to_string())?;
    let bytes = &bytes[..count];
    let valid = match target {
        "x86_64-pc-windows-msvc" if bytes.starts_with(b"MZ") && bytes.len() >= 64 => {
            let offset = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
            bytes
                .get(offset..offset + 6)
                .is_some_and(|header| header == b"PE\0\0\x64\x86")
        }
        "aarch64-apple-darwin" => {
            if bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]) {
                bytes.get(4..8) == Some(&[12, 0, 0, 1])
            } else if bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) && bytes.len() >= 8 {
                let count = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
                (0..count.min(32))
                    .any(|i| bytes.get(8 + i * 20..12 + i * 20) == Some(&[1, 0, 0, 12]))
            } else {
                false
            }
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "Wrong architecture for {target}: {}",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload() -> (tempfile::TempDir, String) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("android-tools");
        fs::create_dir(&root).unwrap();
        let mut files = Vec::new();
        for name in ["adb", "mitmdump", "worker", "guardian", "addon"] {
            let mut bytes = vec![0xcf, 0xfa, 0xed, 0xfe, 12, 0, 0, 1];
            bytes.extend([0; 24]);
            fs::write(root.join(name), &bytes).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o755)).unwrap();
            }
            files.push(serde_json::json!({"path":name,"kind":"file","bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}));
        }
        let bytes = serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"target":"aarch64-apple-darwin","adbVersion":"37.0.1","mitmproxyVersion":"12.2.3","tools":{"adb":"adb","mitmdump":"mitmdump","worker":"worker","guardian":"guardian","addon":"addon"},"files":files})).unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        fs::write(root.join("payload.json"), bytes).unwrap();
        (temp, digest)
    }
    #[test]
    fn admits_complete_pinned_payload() {
        let (temp, digest) = payload();
        assert!(resolve_verified(temp.path(), &digest, "aarch64-apple-darwin").is_ok());
    }
    #[test]
    fn rejects_tampered_payload() {
        let (temp, digest) = payload();
        fs::write(temp.path().join("android-tools/adb"), b"tampered").unwrap();
        assert!(
            resolve_verified(temp.path(), &digest, "aarch64-apple-darwin")
                .unwrap_err()
                .contains("integrity")
        );
    }
    #[test]
    fn rejects_changed_manifest() {
        let (temp, digest) = payload();
        fs::write(temp.path().join("android-tools/payload.json"), b"{}").unwrap();
        assert!(
            resolve_verified(temp.path(), &digest, "aarch64-apple-darwin")
                .unwrap_err()
                .contains("manifest integrity")
        );
    }
    #[test]
    fn missing_payload_never_discovers_external_tools() {
        let temp = tempfile::tempdir().unwrap();
        assert!(
            resolve_verified(temp.path(), "unused", "aarch64-apple-darwin")
                .unwrap_err()
                .contains("missing")
        );
    }
    #[test]
    fn rejects_missing_worker_and_wrong_host() {
        let (temp, digest) = payload();
        assert!(resolve_verified(temp.path(), &digest, "x86_64-pc-windows-msvc").is_err());
        fs::remove_file(temp.path().join("android-tools/worker")).unwrap();
        assert!(
            resolve_verified(temp.path(), &digest, "aarch64-apple-darwin")
                .unwrap_err()
                .contains("Missing bundled component worker")
        );
    }
    #[test]
    fn rejects_escaping_paths() {
        for value in ["../adb", "/adb", "a/../adb", "C:adb", "a\\adb", ""] {
            assert!(relative(value).is_err());
        }
    }
}
