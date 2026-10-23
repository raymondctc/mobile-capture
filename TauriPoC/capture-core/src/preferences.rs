use crate::{files, lease, model::*};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub output_root: PathBuf,
    pub limits: Limits,
    pub network_policy: NetworkPolicy,
    pub adb_override: Option<PathBuf>,
    pub mitmdump_override: Option<PathBuf>,
}

impl Preferences {
    pub fn defaults(output_root: PathBuf) -> Self {
        Self {
            output_root,
            limits: Limits::default(),
            network_policy: NetworkPolicy::default(),
            adb_override: None,
            mitmdump_override: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.limits.validate()?;
        self.network_policy.validate()?;
        if !self.output_root.is_absolute() {
            return Err("Choose an absolute capture storage directory".into());
        }
        for path in [&self.adb_override, &self.mitmdump_override]
            .into_iter()
            .flatten()
        {
            if !path.is_absolute() || path.as_os_str().is_empty() {
                return Err("Explicit tool overrides must be absolute executable paths".into());
            }
        }
        Ok(())
    }
}

pub fn load(path: &Path, output_root: PathBuf) -> Result<Preferences> {
    let parent = path.parent().ok_or("Preferences have no directory")?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid preferences file name")?;
    let mut file = match files::open(parent, name) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Preferences::defaults(output_root))
        }
        Err(error) => return Err(error.to_string()),
    };
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Preferences are not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("Preferences exceed 64 KiB".into());
    }
    let preferences: Preferences =
        serde_json::from_slice(&bytes).map_err(|e| format!("Preferences are invalid. {e}"))?;
    preferences.validate()?;
    Ok(preferences)
}

pub fn store(path: &Path, preferences: &Preferences) -> Result<()> {
    preferences.validate()?;
    lease::private_directory(path.parent().ok_or("Preferences have no directory")?)?;
    lease::write(path, preferences)
}

pub fn prepare_directory(path: &Path) -> Result<()> {
    lease::private_directory(path)
}

pub fn executable_override(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err("Tool override is not absolute".into());
    }
    crate::process::absolute_executable(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_round_trip_and_refuse_relative_overrides_or_unknown_fields() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("preferences.json");
        let mut preferences = Preferences::defaults(temporary.path().join("sessions"));
        store(&path, &preferences).unwrap();
        assert_eq!(
            load(&path, temporary.path().into()).unwrap().limits.bytes,
            200000000
        );
        preferences.adb_override = Some("adb".into());
        assert!(store(&path, &preferences).is_err());
        assert!(load(&path, temporary.path().into())
            .unwrap()
            .adb_override
            .is_none());
        std::fs::write(&path, b"{\"unknown\":true}").unwrap();
        assert!(load(&path, temporary.path().into()).is_err());
    }
}
