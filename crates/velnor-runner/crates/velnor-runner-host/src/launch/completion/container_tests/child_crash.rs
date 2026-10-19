//! Bounded child-process interruption helpers.

use crate::error::HostError;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::time::Duration;

pub(super) fn run_partial_capture_child() -> Result<(), HostError> {
    const CHILD_PATH: &str = "VELNOR_TEST_PARTIAL_CAPTURE_PATH";
    const READY_PATH: &str = "VELNOR_TEST_PARTIAL_CAPTURE_READY";
    const ENGINE_ROOT: &str = "VELNOR_TEST_PARTIAL_CAPTURE_ENGINE_ROOT";
    const ENGINE_ID: &str = "VELNOR_TEST_PARTIAL_CAPTURE_ENGINE_ID";
    let (Ok(path), Ok(ready), Ok(root), Ok(engine_id)) = (
        std::env::var(CHILD_PATH),
        std::env::var(READY_PATH),
        std::env::var(ENGINE_ROOT),
        std::env::var(ENGINE_ID),
    ) else {
        return Ok(());
    };
    let _engine_guard = crate::daemon_lock::EngineLineageGuard::acquire_at_for_tests(
        &engine_id,
        std::path::Path::new(&root),
    )
    .map_err(|_| HostError::Lock)?;
    let mut partial = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| HostError::Path)?;
    partial
        .write_all(b"incomplete child capture")
        .map_err(|_| HostError::Path)?;
    partial.sync_all().map_err(|_| HostError::Path)?;
    std::fs::write(ready, b"ready").map_err(|_| HostError::Path)?;
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

pub(super) fn spawn_child_with_partial_file(
    partial_path: &std::path::Path,
    scratch: &std::path::Path,
    engine_id: &str,
) -> Result<Vec<u8>, HostError> {
    use std::process::{Command, Stdio};

    const CHILD_PATH: &str = "VELNOR_TEST_PARTIAL_CAPTURE_PATH";
    const READY_PATH: &str = "VELNOR_TEST_PARTIAL_CAPTURE_READY";
    const ENGINE_ROOT: &str = "VELNOR_TEST_PARTIAL_CAPTURE_ENGINE_ROOT";
    const ENGINE_ID: &str = "VELNOR_TEST_PARTIAL_CAPTURE_ENGINE_ID";
    let ready_path = scratch.join("child-ready");
    let current_thread = std::thread::current();
    let test_name = current_thread.name().ok_or(HostError::Journal)?;
    let mut child = Command::new(std::env::current_exe().map_err(|_| HostError::Path)?)
        .arg("--exact")
        .arg(test_name)
        .env(CHILD_PATH, partial_path)
        .env(READY_PATH, &ready_path)
        .env(ENGINE_ROOT, scratch)
        .env(ENGINE_ID, engine_id)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| HostError::Path)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !ready_path.exists() {
        if let Some(status) = child.try_wait().map_err(|_| HostError::Path)? {
            return Err(if status.success() {
                HostError::Journal
            } else {
                HostError::Path
            });
        }
        if std::time::Instant::now() >= deadline {
            let _killed = child.kill();
            let _waited = child.wait();
            return Err(HostError::DockerTimeout);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !matches!(
        crate::daemon_lock::EngineLineageGuard::acquire_at_for_tests(engine_id, scratch),
        Err(HostError::Lock)
    ) {
        let _killed = child.kill();
        let _waited = child.wait();
        return Err(HostError::Journal);
    }
    child.kill().map_err(|_| HostError::Path)?;
    let status = child.wait().map_err(|_| HostError::Path)?;
    if status.success() {
        return Err(HostError::Journal);
    }
    std::fs::read(partial_path).map_err(|_| HostError::Path)
}
