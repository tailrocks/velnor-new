use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path},
    time::Instant,
};

use rustix::fs::{Mode, OFlags, open, openat};
use serde::{Deserialize, Serialize};

use velnor_resource_probe::ProbeError;

use super::{
    DECISION_FILE, DECISION_LIMIT, DECISION_POLL_INTERVAL, Decision, REQUEST_FILE, RUNNER_UID,
    validation,
};

pub(super) fn open_trusted_directory(
    path: &str,
    owner: u32,
    private_owner_only: bool,
) -> Result<File, ProbeError> {
    let path = Path::new(path);
    let mut components = Vec::new();
    for (index, component) in path.components().enumerate() {
        match (index, component) {
            (0, Component::RootDir) => {}
            (_, Component::Normal(name)) => {
                components.push(name.to_str().ok_or(ProbeError::Invalid("job_hook_path"))?);
            }
            _ => return Err(ProbeError::Invalid("job_hook_path")),
        }
    }
    if !path.is_absolute() || components.is_empty() {
        return Err(ProbeError::Invalid("job_hook_path"));
    }
    let mut current = File::from(
        open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| ProbeError::Invalid("job_hook_directory"))?,
    );
    validate_directory_owner(&current, 0, false)?;
    for (index, component) in components.iter().enumerate() {
        let next = openat(
            &current,
            *component,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| ProbeError::Invalid("job_hook_directory"))?;
        current = File::from(next);
        let final_component = index + 1 == components.len();
        validate_directory_owner(
            &current,
            if final_component { owner } else { 0 },
            final_component && private_owner_only,
        )?;
    }
    Ok(current)
}

fn validate_directory_owner(
    directory: &File,
    owner: u32,
    private_owner_only: bool,
) -> Result<(), ProbeError> {
    let metadata = directory
        .metadata()
        .map_err(|_| ProbeError::Invalid("job_hook_directory"))?;
    let mode = metadata.mode() & 0o777;
    if !metadata.is_dir()
        || metadata.uid() != owner
        || mode & 0o022 != 0
        || (private_owner_only && mode & 0o077 != 0)
        || (!private_owner_only && mode & 0o005 != 0o005)
    {
        return Err(ProbeError::Invalid("job_hook_directory_owner"));
    }
    Ok(())
}

pub(super) fn read_json_at<T: for<'de> Deserialize<'de> + Serialize>(
    directory: &File,
    name: &str,
    owner: u32,
    limit: usize,
    readonly: bool,
) -> Result<T, ProbeError> {
    let file = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ProbeError::Invalid("job_hook_file"))?;
    let file = File::from(file);
    let metadata = file
        .metadata()
        .map_err(|_| ProbeError::Invalid("job_hook_file"))?;
    let mode = metadata.mode() & 0o777;
    if !metadata.is_file()
        || metadata.uid() != owner
        || (readonly && mode & 0o222 != 0)
        || (readonly && mode & 0o004 == 0)
        || (!readonly && mode & 0o077 != 0)
        || metadata.len() > limit as u64
    {
        return Err(ProbeError::Invalid("job_hook_file_owner"));
    }
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("job_hook_file_read"))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge("job_hook_file"));
    }
    let value: T =
        serde_json::from_slice(&bytes).map_err(|_| ProbeError::Invalid("job_hook_json"))?;
    let canonical = validation::canonical_json(&value, limit, "job_hook_json")?;
    if canonical != bytes {
        return Err(ProbeError::Invalid("job_hook_json_canonical"));
    }
    Ok(value)
}

pub(super) fn write_request_at(
    inbox: &File,
    inbox_path: &Path,
    bytes: &[u8],
    challenge: &str,
) -> Result<(), ProbeError> {
    let temporary = format!("request.{challenge}.tmp");
    let temp_fd = openat(
        inbox,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| ProbeError::Invalid("job_hook_request_replay"))?;
    let mut temp_file = File::from(temp_fd);
    temp_file
        .write_all(bytes)
        .and_then(|()| temp_file.sync_all())
        .map_err(|_| ProbeError::Invalid("job_hook_request_write"))?;
    let temp_path = inbox_path.join(&temporary);
    let request_path = inbox_path.join(REQUEST_FILE);
    fs::hard_link(&temp_path, &request_path)
        .map_err(|_| ProbeError::Invalid("job_hook_request_replay"))?;
    inbox
        .sync_all()
        .map_err(|_| ProbeError::Invalid("job_hook_request_publish"))?;
    fs::remove_file(&temp_path).map_err(|_| ProbeError::Invalid("job_hook_request_publish"))?;
    Ok(())
}

pub(super) fn wait_for_decision(
    control: &File,
    control_path: &Path,
    deadline: Instant,
) -> Result<Decision, ProbeError> {
    loop {
        if Instant::now() >= deadline {
            return Err(ProbeError::Invalid("job_hook_decision_timeout"));
        }
        match read_json_at::<Decision>(control, DECISION_FILE, 0, DECISION_LIMIT, true) {
            Ok(decision) => return Ok(decision),
            Err(ProbeError::Invalid("job_hook_file"))
                if !control_path.join(DECISION_FILE).exists() =>
            {
                let remaining = deadline.saturating_duration_since(Instant::now());
                std::thread::sleep(remaining.min(DECISION_POLL_INTERVAL));
            }
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn read_bounded_path(path: &Path, limit: usize) -> Result<Vec<u8>, ProbeError> {
    if !path.is_absolute() {
        return Err(ProbeError::Invalid("job_hook_event_path"));
    }
    let fd = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ProbeError::Invalid("job_hook_event_path"))?;
    let file = File::from(fd);
    let metadata = file
        .metadata()
        .map_err(|_| ProbeError::Invalid("job_hook_event_file"))?;
    if !metadata.is_file()
        || metadata.uid() != RUNNER_UID
        || metadata.mode() & 0o022 != 0
        || metadata.len() > limit as u64
    {
        return Err(ProbeError::Invalid("job_hook_event_file"));
    }
    let mut bytes = Vec::with_capacity(limit.min(4096));
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProbeError::Invalid("job_hook_event_read"))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge("job_hook_event"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod fifo_tests {
    use std::{fs, os::unix::fs::FileTypeExt, path::PathBuf, sync::mpsc, thread, time::Duration};

    use rustix::{
        fs::{Mode, open},
        process::geteuid,
    };

    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "velnor-hook-fifo-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create FIFO test directory");
            Self(path)
        }

        fn fifo(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            let status = std::process::Command::new("mkfifo")
                .arg(&path)
                .status()
                .expect("create FIFO");
            assert!(status.success());
            assert!(
                fs::symlink_metadata(&path)
                    .expect("inspect FIFO")
                    .file_type()
                    .is_fifo()
            );
            path
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _cleanup_result = fs::remove_dir_all(&self.0);
        }
    }

    fn require_fast_error(operation: impl FnOnce() -> Result<(), ProbeError> + Send + 'static) {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _send_result = sender.send(operation());
        });
        let result = receiver
            .recv_timeout(Duration::from_millis(500))
            .expect("FIFO open blocked without a writer");
        assert!(result.is_err());
    }

    #[test]
    fn control_file_fifo_returns_without_a_writer() {
        let directory = TestDirectory::new();
        let file = directory.fifo(DECISION_FILE);
        let descriptor = open(
            &directory.0,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .expect("open control directory");
        require_fast_error(move || {
            read_json_at::<Decision>(
                &File::from(descriptor),
                DECISION_FILE,
                geteuid().as_raw(),
                DECISION_LIMIT,
                true,
            )
            .map(|_| ())
        });
        assert!(file.exists());
    }

    #[test]
    fn event_file_fifo_returns_without_a_writer() {
        let directory = TestDirectory::new();
        let path = directory.fifo("event.json");
        require_fast_error(move || read_bounded_path(&path, 1024).map(|_| ()));
    }
}
