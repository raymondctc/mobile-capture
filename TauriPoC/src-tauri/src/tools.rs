use std::{ffi::OsString, path::PathBuf};

const ADB: &str = if cfg!(windows) { "adb.exe" } else { "adb" };
const MITMDUMP: &str = if cfg!(windows) {
    "mitmdump.exe"
} else {
    "mitmdump"
};

#[derive(Default)]
pub(crate) struct DiscoveryEnv {
    pub(crate) path: Vec<PathBuf>,
    pub(crate) android_home: Option<PathBuf>,
    pub(crate) android_sdk_root: Option<PathBuf>,
    pub(crate) home: Option<PathBuf>,
    pub(crate) local_app_data: Option<PathBuf>,
}

impl DiscoveryEnv {
    pub(crate) fn from_process() -> Self {
        Self {
            path: std::env::var_os("PATH")
                .map(|value| std::env::split_paths(&value).collect())
                .unwrap_or_default(),
            android_home: nonempty_path(std::env::var_os("ANDROID_HOME")),
            android_sdk_root: nonempty_path(std::env::var_os("ANDROID_SDK_ROOT")),
            home: nonempty_path(std::env::var_os(if cfg!(windows) {
                "USERPROFILE"
            } else {
                "HOME"
            })),
            local_app_data: nonempty_path(std::env::var_os("LOCALAPPDATA")),
        }
    }
}

fn nonempty_path(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

pub(crate) fn installed_adb(environment: &DiscoveryEnv) -> Option<PathBuf> {
    adb_in(environment, &known_directories())
}

pub(crate) fn installed_mitmdump(environment: &DiscoveryEnv) -> Option<PathBuf> {
    let mut directories = Vec::new();
    if cfg!(unix) && !cfg!(target_os = "macos") {
        if let Some(home) = &environment.home {
            directories.push(home.join(".local/bin"));
        }
    }
    directories.extend(known_directories());
    mitmdump_in(environment, &directories)
}

fn known_directories() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec!["/opt/homebrew/bin".into(), "/usr/local/bin".into()]
    } else if cfg!(unix) {
        vec!["/usr/local/bin".into()]
    } else {
        Vec::new()
    }
}

fn default_sdk(environment: &DiscoveryEnv) -> Option<PathBuf> {
    if cfg!(windows) {
        environment
            .local_app_data
            .clone()
            .or_else(|| {
                environment
                    .home
                    .as_ref()
                    .map(|home| home.join("AppData/Local"))
            })
            .map(|local| local.join("Android/Sdk"))
    } else {
        environment.home.as_ref().map(|home| {
            home.join(if cfg!(target_os = "macos") {
                "Library/Android/sdk"
            } else {
                "Android/Sdk"
            })
        })
    }
}

fn adb_in(environment: &DiscoveryEnv, directories: &[PathBuf]) -> Option<PathBuf> {
    let mut candidates: Vec<_> = environment
        .path
        .iter()
        .map(|directory| directory.join(ADB))
        .collect();
    for sdk in environment
        .android_home
        .iter()
        .chain(&environment.android_sdk_root)
        .chain(default_sdk(environment).iter())
    {
        candidates.push(sdk.join("platform-tools").join(ADB));
    }
    candidates.extend(directories.iter().map(|directory| directory.join(ADB)));
    first_executable(candidates)
}

fn mitmdump_in(environment: &DiscoveryEnv, directories: &[PathBuf]) -> Option<PathBuf> {
    first_executable(
        environment
            .path
            .iter()
            .chain(directories)
            .map(|directory| directory.join(MITMDUMP)),
    )
}

fn first_executable(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find_map(|candidate| {
        let path = candidate.canonicalize().ok()?;
        let metadata = path.metadata().ok()?;
        if !path.is_absolute() || !metadata.is_file() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                return None;
            }
        }
        Some(path)
    })
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
            ..DiscoveryEnv::default()
        };

        assert_eq!(installed_adb(&environment), Some(adb));
    }

    #[test]
    fn path_wins_over_sdk_and_known_directories() {
        let fixture = Fixture::new();
        let adb = fixture.executable(Path::new("path-bin").join(ADB));
        let mitmdump = fixture.executable(Path::new("path-bin").join(MITMDUMP));
        fixture.executable(Path::new("second-bin").join(ADB));
        fixture.executable(Path::new("second-bin").join(MITMDUMP));
        fixture.executable(Path::new("sdk/platform-tools").join(ADB));
        fixture.executable(Path::new("known-bin").join(ADB));
        fixture.executable(Path::new("known-bin").join(MITMDUMP));
        let environment = DiscoveryEnv {
            path: vec![fixture.path("path-bin"), fixture.path("second-bin")],
            android_home: Some(fixture.path("sdk")),
            ..DiscoveryEnv::default()
        };
        let directories = [fixture.path("known-bin")];

        assert_eq!(adb_in(&environment, &directories), Some(adb));
        assert_eq!(installed_mitmdump(&environment), Some(mitmdump.clone()));
        assert_eq!(mitmdump_in(&environment, &directories), Some(mitmdump));
    }

    #[test]
    fn android_home_precedes_sdk_root_then_default() {
        let fixture = Fixture::new();
        let environment = DiscoveryEnv {
            android_home: Some(fixture.path("configured-sdk")),
            android_sdk_root: Some(fixture.path("legacy-sdk")),
            home: Some(fixture.path("home")),
            ..DiscoveryEnv::default()
        };
        let default_root = if cfg!(windows) {
            "home/AppData/Local/Android/Sdk"
        } else if cfg!(target_os = "macos") {
            "home/Library/Android/sdk"
        } else {
            "home/Android/Sdk"
        };
        let default = fixture.executable(Path::new(default_root).join("platform-tools").join(ADB));
        let legacy = fixture.executable(Path::new("legacy-sdk/platform-tools").join(ADB));
        let configured = fixture.executable(Path::new("configured-sdk/platform-tools").join(ADB));

        assert_eq!(adb_in(&environment, &[]), Some(configured.clone()));
        fs::remove_file(configured).unwrap();
        assert_eq!(adb_in(&environment, &[]), Some(legacy.clone()));
        fs::remove_file(legacy).unwrap();
        assert_eq!(adb_in(&environment, &[]), Some(default));
    }

    #[test]
    fn invalid_path_candidates_fall_through_to_executable_files() {
        let fixture = Fixture::new();
        let mut path = vec![fixture.path("missing-bin"), fixture.path("directory-bin")];
        #[cfg(unix)]
        path.push(fixture.path("nonexecutable-bin"));
        path.push(fixture.path("valid-bin"));
        let environment = DiscoveryEnv {
            path,
            ..DiscoveryEnv::default()
        };
        for name in [ADB, MITMDUMP] {
            fs::create_dir_all(fixture.path(Path::new("directory-bin").join(name))).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let file = fixture.executable(Path::new("nonexecutable-bin").join(name));
                fs::set_permissions(file, fs::Permissions::from_mode(0o644)).unwrap();
            }
            fixture.executable(Path::new("valid-bin").join(name));
        }

        assert_eq!(
            adb_in(&environment, &[]),
            Some(fixture.path("valid-bin").join(ADB).canonicalize().unwrap())
        );
        assert_eq!(
            mitmdump_in(&environment, &[]),
            Some(
                fixture
                    .path("valid-bin")
                    .join(MITMDUMP)
                    .canonicalize()
                    .unwrap()
            )
        );
    }

    #[test]
    fn known_directories_find_installed_mitmdump() {
        let fixture = Fixture::new();
        let mitmdump = fixture.executable(Path::new("opt/homebrew/bin").join(MITMDUMP));

        assert_eq!(
            mitmdump_in(
                &DiscoveryEnv::default(),
                &[fixture.path("opt/homebrew/bin")]
            ),
            Some(mitmdump),
        );
    }

    #[cfg(unix)]
    #[test]
    fn broken_symlink_falls_through_to_canonical_executable_target() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let target = fixture.executable("cellar/mitmdump");
        fs::create_dir_all(fixture.path("broken-bin")).unwrap();
        fs::create_dir_all(fixture.path("homebrew-bin")).unwrap();
        symlink(fixture.path("missing"), fixture.path("broken-bin/mitmdump")).unwrap();
        symlink(&target, fixture.path("homebrew-bin/mitmdump")).unwrap();
        let environment = DiscoveryEnv {
            path: vec![fixture.path("broken-bin")],
            ..DiscoveryEnv::default()
        };

        assert_eq!(
            mitmdump_in(&environment, &[fixture.path("homebrew-bin")]),
            Some(target)
        );
    }

    #[cfg(windows)]
    #[test]
    fn local_app_data_sdk_precedes_userprofile_default() {
        let fixture = Fixture::new();
        let local = fixture.executable("local/Android/Sdk/platform-tools/adb.exe");
        fixture.executable("home/AppData/Local/Android/Sdk/platform-tools/adb.exe");
        let environment = DiscoveryEnv {
            home: Some(fixture.path("home")),
            local_app_data: Some(fixture.path("local")),
            ..DiscoveryEnv::default()
        };

        assert_eq!(installed_adb(&environment), Some(local));
    }

    #[test]
    fn missing_tools_remain_absent() {
        let environment = DiscoveryEnv::default();
        assert_eq!(adb_in(&environment, &[]), None);
        assert_eq!(mitmdump_in(&environment, &[]), None);
    }

    #[test]
    fn empty_sdk_variables_are_absent() {
        assert_eq!(nonempty_path(Some(OsString::new())), None);
        assert_eq!(nonempty_path(None), None);
        assert_eq!(
            nonempty_path(Some("sdk".into())),
            Some(PathBuf::from("sdk"))
        );
    }
}
