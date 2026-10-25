use crate::{files, library::RetainedBody, model::Result, save};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn base64_body(body: &RetainedBody) -> Result<String> {
    clipboard_base64_length(body.bytes.len())?;
    Ok(STANDARD.encode(&body.bytes))
}

fn clipboard_base64_length(bytes: usize) -> Result<usize> {
    let length = bytes
        .checked_add(2)
        .and_then(|n| n.checked_div(3))
        .and_then(|n| n.checked_mul(4))
        .ok_or("Clipboard size overflow")?;
    if length > 128 * 1024 * 1024 {
        return Err("Clipboard text exceeds 128 MiB".into());
    }
    Ok(length)
}

pub fn save_body(
    body: &RetainedBody,
    destination: &Path,
    protected: &[PathBuf],
) -> Result<PathBuf> {
    let parent = fs::canonicalize(destination.parent().ok_or("Destination has no directory")?)
        .map_err(|e| e.to_string())?;
    let target = parent.join(
        destination
            .file_name()
            .ok_or("Destination has no filename")?,
    );
    for root in std::iter::once(&body.origin).chain(protected) {
        let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
        if target.starts_with(root) {
            return Err("Save the body outside capture evidence folders".into());
        }
    }
    files::refuse_link(&target)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&parent).map_err(|e| e.to_string())?;
    temporary
        .write_all(&body.bytes)
        .map_err(|e| e.to_string())?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|e| e.to_string())?;
    save::publish(temporary.path(), &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_limit_is_checked_before_encoding_allocation() {
        assert_eq!(clipboard_base64_length(0).unwrap(), 0);
        assert_eq!(
            clipboard_base64_length(100_663_296).unwrap(),
            128 * 1024 * 1024
        );
        assert!(clipboard_base64_length(100_663_297).is_err());
        assert!(clipboard_base64_length(usize::MAX).is_err());
    }
    #[test]
    fn complete_base64_and_atomic_save_preserve_retained_bytes() {
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("evidence");
        fs::create_dir(&origin).unwrap();
        let body = RetainedBody {
            bytes: (0..65537).map(|i| (i % 256) as u8).collect(),
            origin: origin.clone(),
        };
        assert_eq!(
            STANDARD.decode(base64_body(&body).unwrap()).unwrap(),
            body.bytes
        );
        let target = root.path().join("body.bin");
        fs::write(&target, b"previous").unwrap();
        save_body(&body, &target, &[]).unwrap();
        assert_eq!(fs::read(&target).unwrap(), body.bytes);
        assert!(save_body(&body, &origin.join("raw.bin"), &[]).is_err());
        assert!(save_body(&body, &target, &[root.path().to_owned()]).is_err());
        assert_eq!(fs::read(target).unwrap(), body.bytes);
    }
    #[cfg(unix)]
    #[test]
    fn aliases_and_symlinks_cannot_overwrite_evidence_or_destination() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("evidence");
        fs::create_dir(&origin).unwrap();
        let body = RetainedBody {
            bytes: b"new".to_vec(),
            origin: origin.clone(),
        };
        let alias = root.path().join("alias");
        symlink(&origin, &alias).unwrap();
        assert!(save_body(&body, &alias.join("body.bin"), &[]).is_err());
        let target = root.path().join("target");
        fs::write(&target, b"previous").unwrap();
        let link = root.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(save_body(&body, &link, &[]).is_err());
        assert_eq!(fs::read(target).unwrap(), b"previous");
    }
}
