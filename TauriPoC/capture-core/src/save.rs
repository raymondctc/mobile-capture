use crate::{files, lease, model::Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const CANCELLED: &str = "Save cancelled before publication";
pub(crate) const CHUNK: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveProgress {
    pub stage: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub cancel_allowed: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CancelOutcome {
    Accepted,
    AlreadyFinished,
    PublicationCommitted,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    Validate,
    Normalize,
    Archive,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum State {
    Running {
        stage: Stage,
        completed: u64,
        total: Option<u64>,
    },
    Cancelled,
    Publishing,
    Published,
    Failed,
}

#[derive(Serialize, Deserialize)]
struct JobState {
    version: u32,
    #[serde(flatten)]
    state: State,
}

#[derive(Clone)]
pub struct SaveHandle {
    session: PathBuf,
    directory: PathBuf,
    id: String,
}

impl SaveHandle {
    pub(crate) fn create(session: &Path) -> Result<Self> {
        let session = fs::canonicalize(session).map_err(|e| e.to_string())?;
        let exports = session.join("runtime/exports");
        lease::private_directory(&session.join("runtime"))?;
        lease::private_directory(&exports)?;
        let id = lease::random_token()?;
        let directory = exports.join(&id);
        fs::create_dir(&directory).map_err(|e| e.to_string())?;
        lease::private_directory(&directory)?;
        lease::write(
            &directory.join("state.json"),
            &JobState {
                version: 1,
                state: State::Running {
                    stage: Stage::Validate,
                    completed: 0,
                    total: None,
                },
            },
        )?;
        Ok(Self {
            session,
            directory,
            id,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn progress(&self) -> Result<SaveProgress> {
        let state = self.read()?.state;
        let (stage, completed, total, cancel_allowed) = match state {
            State::Running {
                stage,
                completed,
                total,
            } => (
                match stage {
                    Stage::Validate => "validate",
                    Stage::Normalize => "normalize",
                    Stage::Archive => "archive",
                },
                completed,
                total,
                true,
            ),
            State::Cancelled => ("cancelled", 0, None, false),
            State::Publishing => ("publish", 0, None, false),
            State::Published => ("complete", 1, Some(1), false),
            State::Failed => ("failed", 0, None, false),
        };
        Ok(SaveProgress {
            stage: stage.into(),
            completed,
            total,
            cancel_allowed,
        })
    }

    pub fn cancel(&self) -> Result<CancelOutcome> {
        let _gate = self.gate()?;
        match self.read()?.state {
            State::Running { .. } => {
                self.write(State::Cancelled)?;
                Ok(CancelOutcome::Accepted)
            }
            State::Publishing | State::Published => Ok(CancelOutcome::PublicationCommitted),
            State::Cancelled | State::Failed => Ok(CancelOutcome::AlreadyFinished),
        }
    }

    pub(crate) fn open(session: &Path, id: &str) -> Result<Self> {
        if id.len() != 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("Invalid private save identifier".into());
        }
        let session = fs::canonicalize(session).map_err(|e| e.to_string())?;
        let handle = Self {
            directory: session.join("runtime/exports").join(id),
            session,
            id: id.into(),
        };
        handle.read()?;
        Ok(handle)
    }

    pub(crate) fn matches(&self, directory: &Path) -> Result<()> {
        if fs::canonicalize(directory).map_err(|e| e.to_string())? != self.session {
            return Err("Save handle belongs to another admitted session".into());
        }
        Ok(())
    }

    fn read(&self) -> Result<JobState> {
        #[cfg(not(windows))]
        let bytes = files::read(
            &self.session,
            &format!("runtime/exports/{}/state.json", self.id),
        )?
        .ok_or("Private save state is missing")?;
        #[cfg(windows)]
        let bytes = {
            use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
                FILE_SHARE_READ, FILE_SHARE_WRITE,
            };
            let _directory = files::open(&self.session, &format!("runtime/exports/{}", self.id))
                .map_err(|e| e.to_string())?;
            // Polling must not prevent the guardian from atomically replacing progress.
            let file = fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(self.directory.join("state.json"))
                .map_err(|e| e.to_string())?;
            let metadata = file.metadata().map_err(|e| e.to_string())?;
            if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err("Private save state is not a regular file".into());
            }
            let mut bytes = Vec::new();
            file.take(4097)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            bytes
        };
        if bytes.len() > 4096 {
            return Err("Private save state exceeds its bound".into());
        }
        let state: JobState = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if state.version != 1 {
            return Err("Unsupported private save version".into());
        }
        Ok(state)
    }
    fn write(&self, state: State) -> Result<()> {
        lease::write(
            &self.directory.join("state.json"),
            &JobState { version: 1, state },
        )
    }
    fn gate(&self) -> Result<lease::FileLease> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match lease::FileLease::acquire(&self.directory.join("transition.lock")) {
                Ok(gate) => return Ok(gate),
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => thread::sleep(Duration::from_millis(2)),
            }
        }
    }
    pub(crate) fn check(&self) -> Result<()> {
        match self.read()?.state {
            State::Running { .. } => Ok(()),
            State::Cancelled => Err(CANCELLED.into()),
            _ => Err("Save job is already finished or publication has committed".into()),
        }
    }
    pub(crate) fn update(&self, stage: Stage, completed: u64, total: Option<u64>) -> Result<()> {
        let _gate = self.gate()?;
        self.check()?;
        self.write(State::Running {
            stage,
            completed,
            total,
        })
    }
    pub(crate) fn run(&self, export: impl FnOnce() -> Result<PathBuf>) -> Result<PathBuf> {
        let _runner = lease::FileLease::acquire(&self.directory.join("runner.lock"))?;
        match self.read()?.state {
            State::Published => {
                let archive =
                    files::open(&self.session, "capture-package.zip").map_err(|e| e.to_string())?;
                if !archive.metadata().map_err(|e| e.to_string())?.is_file() {
                    return Err("Published archive is not a regular file".into());
                }
                return Ok(self.session.join("capture-package.zip"));
            }
            State::Cancelled => return Err(CANCELLED.into()),
            State::Running { .. } => {}
            _ => return Err("Save job has finished; prepare a new save to retry".into()),
        }
        let result = export().and_then(|path| fs::canonicalize(path).map_err(|e| e.to_string()));
        if result.is_err() {
            let _gate = self.gate()?;
            if matches!(
                self.read()?.state,
                State::Running { .. } | State::Publishing
            ) {
                self.write(State::Failed)?;
            }
        }
        result
    }
    pub(crate) fn publish(&self, source: &Path, target: &Path) -> Result<()> {
        let _gate = self.gate()?;
        self.check()?;
        self.write(State::Publishing)?;
        match publish(source, target) {
            Ok(()) => self.write(State::Published),
            Err(error) => {
                self.write(State::Failed)?;
                Err(error)
            }
        }
    }
}

pub(crate) fn check(control: Option<&SaveHandle>) -> Result<()> {
    control.map_or(Ok(()), SaveHandle::check)
}
pub(crate) fn update(
    control: Option<&SaveHandle>,
    stage: Stage,
    completed: u64,
    total: Option<u64>,
) -> Result<()> {
    control.map_or(Ok(()), |c| c.update(stage, completed, total))
}

pub(crate) struct CheckedReader<'a, R> {
    inner: R,
    control: Option<&'a SaveHandle>,
}
impl<'a, R> CheckedReader<'a, R> {
    pub(crate) fn new(inner: R, control: Option<&'a SaveHandle>) -> Self {
        Self { inner, control }
    }
}
impl<R: Read> Read for CheckedReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        check(self.control).map_err(std::io::Error::other)?;
        let limit = buffer.len().min(CHUNK);
        self.inner.read(&mut buffer[..limit])
    }
}

pub(crate) fn read(
    session: &Path,
    path: &str,
    control: Option<&SaveHandle>,
) -> Result<Option<Vec<u8>>> {
    let file = match files::open(session, path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Refused evidence path {path}: {error}")),
    };
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err(format!("Evidence is not a regular file: {path}"));
    }
    let mut reader = CheckedReader::new(file, control).take(200_000_001);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 200_000_000 {
        return Err("Evidence exceeds the 200 MB evidence file bound".into());
    }
    Ok(Some(bytes))
}

pub(crate) fn hash(bytes: &[u8], control: Option<&SaveHandle>) -> Result<String> {
    let mut hash = Sha256::new();
    for chunk in bytes.chunks(CHUNK) {
        check(control)?;
        hash.update(chunk);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn copy_archive(source: &Path, destination: &Path) -> Result<PathBuf> {
    let source_parent = source.parent().ok_or("Archive has no parent directory")?;
    let source_name = source
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid archive filename")?;
    let mut input = files::open(source_parent, source_name).map_err(|e| e.to_string())?;
    if !input.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Archive is not a regular file".into());
    }
    let parent = fs::canonicalize(destination.parent().ok_or("Destination has no directory")?)
        .map_err(|e| e.to_string())?;
    let target = parent.join(
        destination
            .file_name()
            .ok_or("Destination has no filename")?,
    );
    if fs::canonicalize(source).map_err(|e| e.to_string())? == target {
        return Ok(target);
    }
    files::refuse_link(&target)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&parent).map_err(|e| e.to_string())?;
    let mut expected = Sha256::new();
    let mut buffer = [0; CHUNK];
    loop {
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        expected.update(&buffer[..n]);
        temporary
            .write_all(&buffer[..n])
            .map_err(|e| e.to_string())?;
    }
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|e| e.to_string())?;
    temporary
        .as_file_mut()
        .seek(SeekFrom::Start(0))
        .map_err(|e| e.to_string())?;
    let mut actual = Sha256::new();
    loop {
        let n = temporary.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        actual.update(&buffer[..n]);
    }
    if expected.finalize() != actual.finalize() {
        return Err("Archive copy verification failed".into());
    }
    files::refuse_link(&target)?;
    publish(temporary.path(), &target)?;
    Ok(target)
}

#[cfg(not(windows))]
pub(crate) fn publish(source: &Path, target: &Path) -> Result<()> {
    files::refuse_link(target)?;
    fs::rename(source, target).map_err(|e| e.to_string())
}
#[cfg(windows)]
pub(crate) fn publish(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    files::refuse_link(target)?;
    let from: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn admitted_cancel_preserves_previous_archive_and_staged_source() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("capture-package.zip");
        let staged = root.path().join("new.zip");
        fs::write(&target, b"prior archive").unwrap();
        fs::write(&staged, b"new archive").unwrap();
        let job = SaveHandle::create(root.path()).unwrap();
        job.update(Stage::Archive, 17, Some(42)).unwrap();
        assert_eq!(job.cancel().unwrap(), CancelOutcome::Accepted);
        assert!(job.publish(&staged, &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"prior archive");
        assert_eq!(fs::read(&staged).unwrap(), b"new archive");
        assert_eq!(job.progress().unwrap().stage, "cancelled");
        assert_eq!(job.cancel().unwrap(), CancelOutcome::AlreadyFinished);
    }

    #[test]
    fn cancellation_without_prior_archive_publishes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("capture-package.zip");
        let staged = root.path().join("staged.zip");
        fs::write(&staged, b"staged archive").unwrap();
        let job = SaveHandle::create(root.path()).unwrap();
        assert_eq!(job.cancel().unwrap(), CancelOutcome::Accepted);
        assert!(job.publish(&staged, &target).is_err());
        assert!(!target.exists());
    }

    #[test]
    fn polling_progress_does_not_block_atomic_state_updates() {
        let root = tempfile::tempdir().unwrap();
        let job = SaveHandle::create(root.path()).unwrap();
        let observer = job.clone();
        let polling = thread::spawn(move || loop {
            let progress = observer.progress().unwrap();
            if progress.stage == "cancelled" {
                break;
            }
            thread::yield_now();
        });
        for n in 0..64 {
            job.update(Stage::Archive, n, Some(64)).unwrap();
        }
        assert_eq!(job.cancel().unwrap(), CancelOutcome::Accepted);
        polling.join().unwrap();
    }

    #[test]
    fn cancel_and_publication_race_have_exactly_one_winner() {
        for _ in 0..32 {
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("capture-package.zip");
            let staged = root.path().join("new.zip");
            fs::write(&target, b"prior").unwrap();
            fs::write(&staged, b"new").unwrap();
            let job = SaveHandle::create(root.path()).unwrap();
            let cancel = job.clone();
            let barrier = Arc::new(Barrier::new(2));
            let cancel_barrier = barrier.clone();
            let cancelling = thread::spawn(move || {
                cancel_barrier.wait();
                cancel.cancel().unwrap()
            });
            barrier.wait();
            let published = job.publish(&staged, &target);
            match cancelling.join().unwrap() {
                CancelOutcome::Accepted => {
                    assert!(published.is_err());
                    assert_eq!(fs::read(&target).unwrap(), b"prior");
                }
                CancelOutcome::PublicationCommitted => {
                    assert!(published.is_ok());
                    assert_eq!(fs::read(&target).unwrap(), b"new");
                }
                CancelOutcome::AlreadyFinished => panic!("race had no completion"),
            }
        }
    }

    #[test]
    fn failed_publish_preserves_prior_bytes_and_new_job_retries_idempotently() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("capture-package.zip");
        fs::write(&target, b"prior").unwrap();
        let failed = SaveHandle::create(root.path()).unwrap();
        assert!(failed
            .run(|| {
                failed.publish(&root.path().join("missing.zip"), &target)?;
                Ok(target.clone())
            })
            .is_err());
        assert_eq!(failed.progress().unwrap().stage, "failed");
        assert_eq!(fs::read(&target).unwrap(), b"prior");
        let retry = SaveHandle::create(root.path()).unwrap();
        let staged = root.path().join("new.zip");
        fs::write(&staged, b"new").unwrap();
        retry
            .run(|| {
                retry.publish(&staged, &target)?;
                Ok(target.clone())
            })
            .unwrap();
        assert_eq!(
            retry.run(|| panic!("published save was repeated")).unwrap(),
            fs::canonicalize(target).unwrap()
        );
        assert_eq!(retry.cancel().unwrap(), CancelOutcome::PublicationCommitted);
        assert_eq!(retry.progress().unwrap().stage, "complete");
    }

    #[test]
    fn handles_are_session_bound_and_private_state_is_not_an_evidence_path() {
        let one = tempfile::tempdir().unwrap();
        let two = tempfile::tempdir().unwrap();
        let handle = SaveHandle::create(one.path()).unwrap();
        assert!(handle.matches(two.path()).is_err());
        assert!(SaveHandle::open(one.path(), "../state").is_err());
        assert!(SaveHandle::open(two.path(), handle.id()).is_err());
        assert_eq!(
            SaveHandle::open(one.path(), handle.id()).unwrap().id(),
            handle.id()
        );
        assert!(one
            .path()
            .join("runtime/exports")
            .join(handle.id())
            .join("state.json")
            .is_file());
        assert!(!one.path().join("save.json").exists());
    }

    #[test]
    fn archive_copy_is_atomic_and_rejects_non_regular_targets() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.zip");
        let destination = root.path().join("copy.zip");
        let bytes = vec![19; CHUNK * 3 + 17];
        fs::write(&source, &bytes).unwrap();
        fs::write(&destination, b"previous").unwrap();
        copy_archive(&source, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        assert_eq!(fs::read(&source).unwrap(), bytes);
        let directory = root.path().join("directory.zip");
        fs::create_dir(&directory).unwrap();
        assert!(copy_archive(&source, &directory).is_err());
        assert!(copy_archive(&root.path().join("missing.zip"), &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), bytes);
    }

    #[cfg(unix)]
    #[test]
    fn copy_and_job_admission_refuse_symlinks() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.zip");
        let link = root.path().join("linked.zip");
        fs::write(&source, b"archive").unwrap();
        symlink(&source, &link).unwrap();
        assert!(copy_archive(&link, &root.path().join("copy.zip")).is_err());
        assert!(copy_archive(&source, &link).is_err());
        let handle = SaveHandle::create(root.path()).unwrap();
        let state = handle.directory.join("state.json");
        fs::remove_file(&state).unwrap();
        symlink(&source, &state).unwrap();
        assert!(SaveHandle::open(root.path(), handle.id()).is_err());
    }
}
