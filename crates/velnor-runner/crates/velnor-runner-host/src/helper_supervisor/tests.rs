use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as OsCommand;
use std::time::Duration;

use tokio::sync::oneshot;

use crate::error::HostError;

use super::run_helper;

pub(super) struct TestObserver {
    pub(super) child_pid: Option<oneshot::Sender<u32>>,
    pub(super) cleanup: Option<oneshot::Sender<Result<(), HostError>>>,
    pub(super) finished: Option<oneshot::Sender<Result<(), HostError>>>,
}

async fn run_helper_with_observer(
    helper: &Path,
    input: Vec<u8>,
    observer: TestObserver,
) -> Result<(), HostError> {
    let (cancel_sender, cancel_receiver) = oneshot::channel();
    let task = tokio::spawn(super::supervise(
        helper.to_owned(),
        input,
        cancel_receiver,
        Some(observer),
    ));
    super::await_helper(task, cancel_sender).await
}

struct TestScript {
    directory: PathBuf,
    path: PathBuf,
}

impl TestScript {
    fn new(contents: &str) -> Result<Self, HostError> {
        let directory =
            std::env::temp_dir().join(format!("velnor-helper-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).map_err(|_| HostError::Path)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
        let path = directory.join("helper.sh");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| HostError::Path)?;
        file.write_all(contents.as_bytes())
            .map_err(|_| HostError::Path)?;
        file.sync_all().map_err(|_| HostError::Path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
        Ok(Self { directory, path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestScript {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.directory) {
            eprintln!("test helper directory cleanup failed: {error}");
        }
    }
}

#[tokio::test]
async fn accepts_only_the_exact_success_response() -> Result<(), HostError> {
    let helper =
        TestScript::new("#!/bin/sh\nprintf '%s' '{\"schema\":1,\"result\":\"verified\"}'\n")?;
    run_helper(helper.path(), b"{}".to_vec()).await
}

#[tokio::test]
async fn rejects_success_with_extra_response_fields() -> Result<(), HostError> {
    let helper = TestScript::new(
        "#!/bin/sh\nprintf '%s' '{\"schema\":1,\"result\":\"verified\",\"ok\":true}'\n",
    )?;
    assert_eq!(
        run_helper(helper.path(), b"{}".to_vec()).await,
        Err(HostError::Identity)
    );
    Ok(())
}

#[tokio::test]
async fn cancelling_the_caller_kills_and_reaps_the_helper() -> Result<(), HostError> {
    let helper = TestScript::new("#!/bin/sh\nexec /bin/sleep 30\n")?;
    let (pid_sender, mut pid_receiver) = oneshot::channel();
    let (cleanup_sender, cleanup_receiver) = oneshot::channel();
    let (finished_sender, finished_receiver) = oneshot::channel();
    let path = helper.path().to_owned();
    let task = tokio::spawn(async move {
        run_helper_with_observer(
            &path,
            b"{}".to_vec(),
            TestObserver {
                child_pid: Some(pid_sender),
                cleanup: Some(cleanup_sender),
                finished: Some(finished_sender),
            },
        )
        .await
    });
    let readiness = tokio::time::timeout(Duration::from_secs(2), &mut pid_receiver).await;
    let Ok(Ok(pid)) = readiness else {
        let observed_pid =
            cancel_and_observe(task, pid_receiver, cleanup_receiver, finished_receiver).await?;
        panic!(
            "helper spawn acknowledgment failed ({readiness:?}); supervisor finished cleanup, child observed: {}",
            observed_pid.is_some()
        );
    };
    task.abort();
    match task.await {
        Err(error) if error.is_cancelled() => {}
        _ => return Err(HostError::Identity),
    }
    let (cleanup, finished) = tokio::time::timeout(Duration::from_secs(2), async {
        (cleanup_receiver.await, finished_receiver.await)
    })
    .await
    .map_err(|_| HostError::DockerTimeout)?;
    cleanup.map_err(|_| HostError::Identity)??;
    assert_eq!(
        finished.map_err(|_| HostError::Identity)?,
        Err(HostError::DockerTimeout)
    );
    wait_until_reaped(pid).await
}

#[tokio::test]
async fn failed_spawn_finishes_without_child_cleanup() -> Result<(), HostError> {
    let helper = TestScript::new("#!/bin/sh\nexit 0\n")?;
    fs::remove_file(helper.path()).map_err(|_| HostError::Path)?;
    let (pid_sender, pid_receiver) = oneshot::channel();
    let (cleanup_sender, cleanup_receiver) = oneshot::channel();
    let (finished_sender, finished_receiver) = oneshot::channel();
    let result = run_helper_with_observer(
        helper.path(),
        b"{}".to_vec(),
        TestObserver {
            child_pid: Some(pid_sender),
            cleanup: Some(cleanup_sender),
            finished: Some(finished_sender),
        },
    )
    .await;
    assert_eq!(result, Err(HostError::Identity));
    assert!(pid_receiver.await.is_err());
    assert!(cleanup_receiver.await.is_err());
    assert_eq!(finished_receiver.await, Ok(Err(HostError::Identity)));
    Ok(())
}

async fn cancel_and_observe(
    task: tokio::task::JoinHandle<Result<(), HostError>>,
    mut pid_receiver: oneshot::Receiver<u32>,
    cleanup_receiver: oneshot::Receiver<Result<(), HostError>>,
    finished_receiver: oneshot::Receiver<Result<(), HostError>>,
) -> Result<Option<u32>, HostError> {
    task.abort();
    let caller_cancelled = matches!(task.await, Err(error) if error.is_cancelled());
    let (cleanup, finished) = tokio::time::timeout(Duration::from_secs(2), async {
        (cleanup_receiver.await, finished_receiver.await)
    })
    .await
    .map_err(|_| HostError::DockerTimeout)?;
    match (
        pid_receiver.try_recv().ok(),
        cleanup,
        finished.map_err(|_| HostError::Identity)?,
        caller_cancelled,
    ) {
        (Some(pid), Ok(Ok(())), Err(_), true) => {
            wait_until_reaped(pid).await?;
            Ok(Some(pid))
        }
        (None, Err(_), Err(_), true) => Ok(None),
        _ => Err(HostError::Identity),
    }
}

async fn wait_until_reaped(pid: u32) -> Result<(), HostError> {
    for _ in 0..200 {
        let status = OsCommand::new("/bin/kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .map_err(|_| HostError::Identity)?;
        if !status.success() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err(HostError::DockerTimeout)
}
