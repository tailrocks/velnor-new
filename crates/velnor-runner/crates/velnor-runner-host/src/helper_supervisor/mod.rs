//! Cancellation-safe boundary to the pinned offline attestation verifier.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

use crate::error::HostError;

mod path;
mod request;
mod snapshot;
pub(super) use request::ChecksumTarget;

const HELPER_DEADLINE: Duration = Duration::from_secs(90);
const KILL_REAP_DEADLINE: Duration = Duration::from_secs(2);
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

struct CancellationGuard(Option<oneshot::Sender<()>>);

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        self.0.take();
    }
}

struct CapturedOutput {
    bytes: Vec<u8>,
    exceeded: bool,
}

pub(super) async fn verify_checksum_target(
    expected_helper_sha256: &[u8; 32],
    target: ChecksumTarget<'_>,
) -> Result<(), HostError> {
    let input = request::encode(&target)?;
    let helper = path::open_verified(expected_helper_sha256)?;
    let helper = snapshot::prepare(target.state_directory, helper, expected_helper_sha256).await?;
    run_helper(helper, input).await
}

async fn run_helper(helper: snapshot::SnapshotLease, input: Vec<u8>) -> Result<(), HostError> {
    let (cancel_sender, cancel_receiver) = oneshot::channel();
    #[cfg(test)]
    let task = tokio::spawn(supervise(helper, input, cancel_receiver, None));
    #[cfg(not(test))]
    let task = tokio::spawn(supervise(helper, input, cancel_receiver));
    await_helper(task, cancel_sender).await
}

async fn await_helper(
    task: JoinHandle<Result<(), HostError>>,
    cancel_sender: oneshot::Sender<()>,
) -> Result<(), HostError> {
    let mut cancellation = CancellationGuard(Some(cancel_sender));
    let result = task.await.map_err(|_| HostError::Identity)?;
    cancellation.0.take();
    result
}

async fn supervise(
    helper: snapshot::SnapshotLease,
    input: Vec<u8>,
    mut cancelled: oneshot::Receiver<()>,
    #[cfg(test)] mut observer: Option<tests::TestObserver>,
) -> Result<(), HostError> {
    let result = supervise_inner(
        &helper,
        input,
        &mut cancelled,
        #[cfg(test)]
        &mut observer,
    )
    .await;
    drop(helper);
    #[cfg(test)]
    if let Some(sender) = observer
        .as_mut()
        .and_then(|observer| observer.finished.take())
    {
        sender.send(result).map_err(|_| HostError::Identity)?;
    }
    result
}

async fn supervise_inner(
    helper: &snapshot::SnapshotLease,
    input: Vec<u8>,
    cancelled: &mut oneshot::Receiver<()>,
    #[cfg(test)] observer: &mut Option<tests::TestObserver>,
) -> Result<(), HostError> {
    let deadline = Instant::now() + HELPER_DEADLINE;
    let mut child = spawn_helper(helper)?;
    let (Some(stdout_pipe), Some(stderr_pipe)) = (child.stdout.take(), child.stderr.take()) else {
        kill_and_reap(&mut child).await?;
        return Err(HostError::Identity);
    };
    let stdout = spawn_capture(stdout_pipe);
    let stderr = spawn_capture(stderr_pipe);
    #[cfg(test)]
    if let Some(sender) = observer
        .as_mut()
        .and_then(|observer| observer.child_pid.take())
    {
        let Some(pid) = child.id() else {
            cleanup_cancelled_child(
                &mut child,
                stdout,
                stderr,
                #[cfg(test)]
                observer,
            )
            .await?;
            return Err(HostError::Identity);
        };
        if sender.send(pid).is_err() {
            cleanup_cancelled_child(
                &mut child,
                stdout,
                stderr,
                #[cfg(test)]
                observer,
            )
            .await?;
            return Err(HostError::Identity);
        }
    }
    let write = write_request(&mut child, &input, deadline, cancelled);
    if write.await.is_err() {
        cleanup_cancelled_child(
            &mut child,
            stdout,
            stderr,
            #[cfg(test)]
            observer,
        )
        .await?;
        return Err(HostError::Identity);
    }
    let status = match wait_child(&mut child, deadline, cancelled).await {
        Ok(status) => status,
        Err(error) => {
            cleanup_cancelled_child(
                &mut child,
                stdout,
                stderr,
                #[cfg(test)]
                observer,
            )
            .await?;
            return Err(error);
        }
    };
    let (stdout, stderr) = collect_output(stdout, stderr, deadline, cancelled).await?;
    if !status.success()
        || stdout.exceeded
        || stderr.exceeded
        || stdout.bytes != br#"{"schema":2,"result":"verified"}"#
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn spawn_helper(helper: &snapshot::SnapshotLease) -> Result<Child, HostError> {
    helper.validate_path()?;
    let mut command = Command::new(helper.path());
    command
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(target_os = "macos")]
    if let Some(value) = std::env::var_os("__CF_USER_TEXT_ENCODING") {
        command.env("__CF_USER_TEXT_ENCODING", value);
    }
    command.spawn().map_err(|_| HostError::Identity)
}

async fn cleanup_cancelled_child(
    child: &mut Child,
    mut stdout: JoinHandle<Result<CapturedOutput, HostError>>,
    mut stderr: JoinHandle<Result<CapturedOutput, HostError>>,
    #[cfg(test)] observer: &mut Option<tests::TestObserver>,
) -> Result<(), HostError> {
    let killed = kill_and_reap(child).await;
    let readers = abort_capture(&mut stdout, &mut stderr).await;
    let cleanup = killed.and(readers);
    #[cfg(test)]
    if let Some(sender) = observer
        .as_mut()
        .and_then(|observer| observer.cleanup.take())
    {
        sender.send(cleanup).map_err(|_| HostError::Identity)?;
    }
    cleanup
}

fn spawn_capture<R>(stream: R) -> JoinHandle<Result<CapturedOutput, HostError>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move { capture(stream).await })
}

async fn capture<R>(mut stream: R) -> Result<CapturedOutput, HostError>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(MAX_OUTPUT_BYTES);
    let mut chunk = [0_u8; 4096];
    let mut exceeded = false;
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| HostError::Identity)?;
        if read == 0 {
            break;
        }
        let room = MAX_OUTPUT_BYTES.saturating_sub(bytes.len());
        let kept = room.min(read);
        bytes.extend_from_slice(&chunk[..kept]);
        exceeded |= kept != read;
    }
    Ok(CapturedOutput { bytes, exceeded })
}

async fn write_request(
    child: &mut Child,
    input: &[u8],
    deadline: Instant,
    cancelled: &mut oneshot::Receiver<()>,
) -> Result<(), HostError> {
    let mut stdin = child.stdin.take().ok_or(HostError::Identity)?;
    tokio::select! {
        _ = cancelled => Err(HostError::DockerTimeout),
        result = timeout_at(deadline, stdin.write_all(input)) => {
            result.map_err(|_| HostError::DockerTimeout)?
                .map_err(|_| HostError::Identity)
        }
    }
}

async fn wait_child(
    child: &mut Child,
    deadline: Instant,
    cancelled: &mut oneshot::Receiver<()>,
) -> Result<std::process::ExitStatus, HostError> {
    tokio::select! {
        _ = cancelled => Err(HostError::DockerTimeout),
        result = timeout_at(deadline, child.wait()) => {
            result.map_err(|_| HostError::DockerTimeout)?
                .map_err(|_| HostError::Identity)
        }
    }
}

async fn kill_and_reap(child: &mut Child) -> Result<(), HostError> {
    if child.try_wait().map_err(|_| HostError::Identity)?.is_some() {
        return Ok(());
    }
    let kill = child.start_kill();
    let waited = tokio::time::timeout(KILL_REAP_DEADLINE, child.wait()).await;
    match waited {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(_)) => Err(HostError::Identity),
        Err(_) => match kill {
            Ok(()) | Err(_) => Err(HostError::DockerTimeout),
        },
    }
}

async fn collect_output(
    mut stdout: JoinHandle<Result<CapturedOutput, HostError>>,
    mut stderr: JoinHandle<Result<CapturedOutput, HostError>>,
    deadline: Instant,
    cancelled: &mut oneshot::Receiver<()>,
) -> Result<(CapturedOutput, CapturedOutput), HostError> {
    tokio::select! {
        _ = cancelled => {
            abort_capture(&mut stdout, &mut stderr).await?;
            Err(HostError::DockerTimeout)
        }
        result = collect_both(&mut stdout, &mut stderr, deadline) => result,
    }
}

async fn collect_both(
    stdout: &mut JoinHandle<Result<CapturedOutput, HostError>>,
    stderr: &mut JoinHandle<Result<CapturedOutput, HostError>>,
    deadline: Instant,
) -> Result<(CapturedOutput, CapturedOutput), HostError> {
    tokio::try_join!(
        join_capture(stdout, deadline),
        join_capture(stderr, deadline)
    )
}

async fn abort_capture(
    stdout: &mut JoinHandle<Result<CapturedOutput, HostError>>,
    stderr: &mut JoinHandle<Result<CapturedOutput, HostError>>,
) -> Result<(), HostError> {
    stdout.abort();
    stderr.abort();
    let stdout = stdout.await;
    let stderr = stderr.await;
    if settled_capture(stdout)? && settled_capture(stderr)? {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

fn settled_capture(
    result: Result<Result<CapturedOutput, HostError>, tokio::task::JoinError>,
) -> Result<bool, HostError> {
    match result {
        Ok(Ok(_)) => Ok(true),
        Ok(Err(error)) => Err(error),
        Err(error) if error.is_cancelled() => Ok(true),
        Err(_) => Err(HostError::Identity),
    }
}

async fn join_capture(
    task: &mut JoinHandle<Result<CapturedOutput, HostError>>,
    deadline: Instant,
) -> Result<CapturedOutput, HostError> {
    timeout_at(deadline, task)
        .await
        .map_err(|_| HostError::DockerTimeout)?
        .map_err(|_| HostError::Identity)?
}

#[cfg(all(test, unix))]
#[path = "tests.rs"]
mod tests;
