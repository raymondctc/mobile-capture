use std::path::PathBuf;

const ADB: &str = if cfg!(windows) { "adb.exe" } else { "adb" };

#[derive(Default)]
pub(crate) struct DiscoveryEnv {
    pub(crate) path: Vec<PathBuf>,
    pub(crate) home: Option<PathBuf>,
}

pub(crate) fn installed_adb(environment: &DiscoveryEnv) -> Option<PathBuf> {
    environment
        .path
        .iter()
        .find_map(|directory| directory.join(ADB).canonicalize().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let directory = std::env::temp_dir().join(format!(
                "mobile-capture-desktop-tools-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&directory).unwrap();
            Self(directory)
        }

        fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
            self.0.join(relative)
        }

        fn executable(&self, relative: impl AsRef<Path>) -> PathBuf {
            let path = self.path(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"synthetic executable, never launched\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            }
            path.canonicalize().unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finder_path_finds_home_android_sdk() {
        let fixture = Fixture::new();
        let adb = fixture.executable("home/Library/Android/sdk/platform-tools/adb");
        let environment = DiscoveryEnv {
            path: vec![fixture.path("system-bin")],
            home: Some(fixture.path("home")),
        };

        assert_eq!(installed_adb(&environment), Some(adb));
    }
}
