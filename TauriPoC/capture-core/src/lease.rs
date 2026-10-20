use crate::model::Result;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub(crate) fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|e| e.to_string())?;
    let info = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !info.is_dir() || info.file_type().is_symlink() {
        return Err("Private directory is not a real directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if info.uid() != unsafe { libc::geteuid() } {
            return Err("Private directory has another owner".into());
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if info.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err("Reparse private directory refused".into());
        }
        secure_windows_directory(path)?;
    }
    Ok(())
}

pub(crate) fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().ok_or("Missing private parent")?;
    private_directory(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    file.persist(path).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let from: Vec<_> = file
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let to: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    #[cfg(unix)]
    File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let parent = path.parent().ok_or("Missing private parent")?;
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("Invalid private filename")?;
    let bytes = crate::files::read(parent, name)?.ok_or("Private state is missing")?;
    serde_json::from_slice(&bytes)
        .map_err(|e| format!("Invalid private state {}: {e}", path.display()))
}

pub(crate) struct FileLease {
    file: File,
}
impl FileLease {
    pub(crate) fn acquire(path: &Path) -> Result<Self> {
        Self::acquire_at(path, 0)
    }
    pub(crate) fn acquire_owner(path: &Path) -> Result<Self> {
        Self::acquire_at(path, 0x7fff_ffff_ffff_fffe)
    }
    fn acquire_at(path: &Path, offset: u64) -> Result<Self> {
        #[cfg(unix)]
        let _ = offset;
        private_directory(path.parent().ok_or("Missing lease directory")?)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(
                windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
            );
        }
        let file = options.open(path).map_err(|e| e.to_string())?;
        let info = file.metadata().map_err(|e| e.to_string())?;
        if !info.is_file() {
            return Err("Lease must be a regular file".into());
        }
        #[cfg(unix)]
        {
            use std::os::{
                fd::AsRawFd,
                unix::fs::{MetadataExt, PermissionsExt},
            };
            if info.uid() != unsafe { libc::geteuid() }
                || info.mode() & 0o022 != 0
                || info.nlink() != 1
            {
                return Err("Lease has unsafe ownership or permissions".into());
            }
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err("Another capture owns this connection or source".into());
            }
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        #[cfg(windows)]
        {
            use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
            use windows_sys::Win32::{
                Storage::FileSystem::{
                    LockFileEx, FILE_ATTRIBUTE_REPARSE_POINT, LOCKFILE_EXCLUSIVE_LOCK,
                    LOCKFILE_FAIL_IMMEDIATELY,
                },
                System::IO::OVERLAPPED,
            };
            if info.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err("Reparse lease refused".into());
            }
            let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
            overlapped.Anonymous.Anonymous.Offset = offset as u32;
            overlapped.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
            if unsafe {
                LockFileEx(
                    file.as_raw_handle() as _,
                    LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                    0,
                    1,
                    0,
                    &mut overlapped,
                )
            } == 0
            {
                return Err("Another capture owns this connection or source".into());
            }
        }
        Ok(Self { file })
    }
    pub(crate) fn prior(&mut self) -> Result<Option<PathBuf>> {
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|e| e.to_string())?;
        let mut text = String::new();
        std::io::Read::by_ref(&mut self.file)
            .take(16385)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        if text.len() > 16384 {
            return Err("Ownership locator exceeds bound".into());
        }
        Ok((!text.trim().is_empty()).then(|| PathBuf::from(text.trim())))
    }
    pub(crate) fn publish(&mut self, path: &Path) -> Result<()> {
        self.file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.set_len(0))
            .and_then(|_| self.file.write_all(path.to_string_lossy().as_bytes()))
            .and_then(|_| self.file.sync_all())
            .map_err(|e| e.to_string())
    }
}

pub(crate) fn random_token() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(windows)]
fn secure_windows_directory(path: &Path) -> Result<()> {
    use std::{
        mem::{size_of, zeroed},
        os::windows::ffi::OsStrExt,
    };
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::*,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let result = (|| {
        let mut length = 0;
        unsafe {
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length);
        }
        if length == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut user = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                user.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let sid = unsafe { (*(user.as_ptr() as *const TOKEN_USER)).User.Sid };
        let acl_bytes = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>()
            + unsafe { GetLengthSid(sid) } as usize;
        let mut storage = vec![0u32; acl_bytes.div_ceil(size_of::<u32>())];
        let acl = storage.as_mut_ptr().cast::<ACL>();
        let mut descriptor: SECURITY_DESCRIPTOR = unsafe { zeroed() };
        let descriptor_ptr = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
        let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let success = unsafe {
            InitializeAcl(acl, acl_bytes as u32, ACL_REVISION) != 0
                && AddAccessAllowedAceEx(
                    acl,
                    ACL_REVISION,
                    OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
                    0x001f01ff,
                    sid,
                ) != 0
                && InitializeSecurityDescriptor(descriptor_ptr, 1) != 0
                && SetSecurityDescriptorDacl(descriptor_ptr, 1, acl, 0) != 0
                && SetFileSecurityW(
                    name.as_ptr(),
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    descriptor_ptr,
                ) != 0
        };
        if success {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().to_string())
        }
    })();
    unsafe {
        CloseHandle(token);
    }
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    };
    #[test]
    fn historical_owner_lock_reuses_inode_tightens_permissions_and_contends() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("owner.lock");
        std::fs::write(&path, b"").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let before = std::fs::metadata(&path).unwrap().ino();
        let native = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(native.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        assert!(FileLease::acquire(&path).is_err());
        drop(native);
        let mut lease = FileLease::acquire(&path).unwrap();
        lease.publish(root.path()).unwrap();
        let info = std::fs::metadata(&path).unwrap();
        assert_eq!(info.ino(), before);
        assert_eq!(info.mode() & 0o777, 0o600);
        assert!(FileLease::acquire(&path).is_err());
        assert_eq!(lease.prior().unwrap().as_deref(), Some(root.path()));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    #[test]
    fn held_owner_lease_leaves_its_locator_readable_and_excludes_other_output_roots() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("owner.lock");
        let mut lease = FileLease::acquire_owner(&path).unwrap();
        lease.publish(root.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            root.path().to_string_lossy()
        );
        assert!(FileLease::acquire_owner(&path).is_err());
    }
}
