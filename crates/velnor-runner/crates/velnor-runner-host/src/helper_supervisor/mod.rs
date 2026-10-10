//! Cancellation-safe boundary to the pinned offline attestation verifier.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

use crate::error::HostError;

mod path;

const HELPER_DEADLINE: Duration = Duration::from_secs(90);
const KILL_REAP_DEADLINE: Duration = Duration::from_secs(2);
const MAX_BUNDLE_BYTES: usize = 2_000_000;
const MAX_CHECKSUM_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BYTES: usize = 3 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Serialize)]
struct Request<'a> {
    schema: u8,
    bundle_base64: String,
    checksum_base64: String,
    expected: Expected<'a>,
    checksum_subject: Subject<'a>,
    target_subject: Subject<'a>,
}

#[derive(Serialize)]
struct Expected<'a> {
    signer: &'a str,
    signer_digest: &'a str,
    source: &'a str,
    source_digest: &'a str,
    source_ref: &'a str,
    build_config: &'a str,
    build_config_digest: &'a str,
}

#[derive(Serialize)]
struct Subject<'a> {
    name: &'a str,
    digest: &'a str,
}

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

pub(super) struct ChecksumTarget<'a> {
    pub(super) bundle: &'a Value,
    pub(super) checksum_bytes: &'a [u8],
    pub(super) source_sha: &'a str,
    pub(super) authority_sha: &'a str,
    pub(super) checksum_digest: &'a str,
    pub(super) target_name: &'a str,
    pub(super) target_digest: &'a str,
}

pub(super) async fn verify_checksum_target(
    expected_helper_sha256: &[u8; 32],
    target: ChecksumTarget<'_>,
) -> Result<(), HostError> {
    if target.checksum_bytes.is_empty()
        || target.checksum_bytes.len() > MAX_CHECKSUM_BYTES
        || target.target_name.is_empty()
    {
        return Err(HostError::Identity);
    }
    let bundle_bytes = serde_json::to_vec(target.bundle).map_err(|_| HostError::Identity)?;
    if bundle_bytes.is_empty() || bundle_bytes.len() > MAX_BUNDLE_BYTES {
        return Err(HostError::Frame);
    }
    let expected = Expected {
        signer: "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main",
        signer_digest: target.authority_sha,
        source: "https://github.com/tailrocks/velnor-new",
        source_digest: target.source_sha,
        source_ref: "refs/heads/main",
        build_config: "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main",
        build_config_digest: target.authority_sha,
    };
    let request = Request {
        schema: 1,
        bundle_base64: base64::engine::general_purpose::STANDARD.encode(bundle_bytes),
        checksum_base64: base64::engine::general_purpose::STANDARD.encode(target.checksum_bytes),
        expected,
        checksum_subject: Subject {
            name: "SHA256SUMS",
            digest: target.checksum_digest,
        },
        target_subject: Subject {
            name: target.target_name,
            digest: target.target_digest,
        },
    };
    let input = serde_json::to_vec(&request).map_err(|_| HostError::Identity)?;
    if input.len() > MAX_REQUEST_BYTES {
        return Err(HostError::Frame);
    }
    let helper = path::open_verified(expected_helper_sha256)?;
    run_helper(&helper, input).await
}

async fn run_helper(helper: &Path, input: Vec<u8>) -> Result<(), HostError> {
    let (cancel_sender, cancel_receiver) = oneshot::channel();
    let task = tokio::spawn(supervise(helper.to_owned(), input, cancel_receiver));
    let mut cancellation = CancellationGuard(Some(cancel_sender));
    let result = task.await.map_err(|_| HostError::Identity)?;
    cancellation.0.take();
    result
}

async fn supervise(
    helper: std::path::PathBuf,
    input: Vec<u8>,
    mut cancelled: oneshot::Receiver<()>,
) -> Result<(), HostError> {
    let deadline = Instant::now() + HELPER_DEADLINE;
    let mut command = Command::new(helper);
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
    let mut child = command.spawn().map_err(|_| HostError::Identity)?;
    let stdout = spawn_capture(child.stdout.take().ok_or(HostError::Identity)?);
    let stderr = spawn_capture(child.stderr.take().ok_or(HostError::Identity)?);
    let write = write_request(&mut child, &input, deadline, &mut cancelled);
    if write.await.is_err() {
        let killed = kill_and_reap(&mut child).await;
        let mut stdout = stdout;
        let mut stderr = stderr;
        let readers = abort_capture(&mut stdout, &mut stderr).await;
        killed?;
        readers?;
        return Err(HostError::Identity);
    }
    let status = match wait_child(&mut child, deadline, &mut cancelled).await {
        Ok(status) => status,
        Err(error) => {
            let killed = kill_and_reap(&mut child).await;
            let mut stdout = stdout;
            let mut stderr = stderr;
            let readers = abort_capture(&mut stdout, &mut stderr).await;
            killed?;
            readers?;
            return Err(error);
        }
    };
    let (stdout, stderr) = collect_output(stdout, stderr, deadline, &mut cancelled).await?;
    if !status.success()
        || stdout.exceeded
        || stderr.exceeded
        || stdout.bytes != br#"{"schema":1,"result":"verified"}"#
    {
        return Err(HostError::Identity);
    }
    Ok(())
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
