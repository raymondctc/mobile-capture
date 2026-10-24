use crate::model::Result;
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
};

pub fn read(root: &Path, relative: &str) -> Result<Option<Vec<u8>>> {
    let mut file = match open(root, relative) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Refused evidence path {relative}: {error}")),
    };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err(format!("Evidence is not a regular file: {relative}"));
    }
    if metadata.len() > 200_000_000 {
        return Err("Evidence exceeds the 200 MB evidence file bound".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(200_000_000 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 200_000_000 {
        return Err("Evidence grew beyond the 200 MB evidence file bound".into());
    }
    Ok(Some(bytes))
}

#[cfg(unix)]
pub(crate) fn open(root: &Path, relative: &str) -> std::io::Result<File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        },
    };
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)?;
    let parts: Vec<_> = relative.split('/').collect();
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || *part == "." || *part == ".." {
            return Err(std::io::Error::other("Invalid relative evidence path"));
        }
        let name = CString::new(std::ffi::OsStr::new(part).as_bytes())?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | libc::O_CLOEXEC
            | if index + 1 < parts.len() {
                libc::O_DIRECTORY
            } else {
                0
            };
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        directory = unsafe { File::from_raw_fd(fd) };
    }
    Ok(directory)
}

#[cfg(windows)]
pub(crate) fn open(root: &Path, relative: &str) -> std::io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let mut guards = Vec::new();
    let mut path = root.to_path_buf();
    let parts: Vec<_> = std::iter::once(None)
        .chain(relative.split('/').map(Some))
        .collect();
    for (index, part) in parts.iter().enumerate() {
        if let Some(part) = part {
            if part.is_empty()
                || *part == "."
                || *part == ".."
                || part.contains('\\')
                || part.contains(':')
            {
                return Err(std::io::Error::other("Invalid relative evidence path"));
            }
            path.push(part);
        }
        // No delete sharing holds each admitted directory in place until the file is open.
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&path)?;
        let metadata = file.metadata()?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (index + 1 < parts.len() && !metadata.is_dir())
        {
            return Err(std::io::Error::other(
                "Symlink or reparse evidence path refused",
            ));
        }
        guards.push(file);
    }
    guards
        .pop()
        .ok_or_else(|| std::io::Error::other("Missing evidence file"))
}

pub fn refuse_link(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err("Reparse publication path refused".into());
                }
            }
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("Publication target is not a regular file".into());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
