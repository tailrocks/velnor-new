//! Size- and digest-bounded release asset download from the GitHub CLI.

use std::ffi::OsString;
use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::oneshot;

use super::super::ProviderFailure;
use super::Workspace;

mod transfer;
use transfer::complete_transfer;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(15);
const DOWNLOAD_CHUNK_BYTES: usize = 64 * 1024;

pub(in crate::launch::resource_probe::provider) struct DownloadVerification {
    pub(in crate::launch::resource_probe::provider) expected_size: u64,
    pub(in crate::launch::resource_probe::provider) max_size: u64,
    pub(in crate::launch::resource_probe::provider) expected_sha256: String,
    pub(in crate::launch::resource_probe::provider) timeout: Duration,
}

pub(super) enum FirstCompletion {
    Copy(Result<(), ProviderFailure>),
    Child(std::io::Result<std::process::ExitStatus>),
    Timeout,
    Cancelled,
}

struct DownloadTask {
    child: Child,
    stdout: Option<ChildStdout>,
    file: tokio::fs::File,
    destination: PathBuf,
    verification: DownloadVerification,
    cancel: oneshot::Receiver<()>,
    ready: oneshot::Sender<Result<(), ProviderFailure>>,
    commit: oneshot::Receiver<()>,
    #[cfg(test)]
    copy_complete: Option<oneshot::Sender<()>>,
}

pub(in crate::launch::resource_probe::provider) async fn download_to_file<I>(
    workspace: &Workspace,
    pat: &str,
    arguments: I,
    destination: &Path,
    expected_size: u64,
    max_size: u64,
    expected_sha256: &str,
) -> Result<(), ProviderFailure>
where
    I: IntoIterator<Item = OsString>,
{
    download_to_file_with_timeout(
        workspace,
        pat,
        arguments,
        destination,
        DownloadVerification {
            expected_size,
            max_size,
            expected_sha256: expected_sha256.to_owned(),
            timeout: DOWNLOAD_TIMEOUT,
        },
    )
    .await
}

pub(in crate::launch::resource_probe::provider) async fn download_to_file_with_timeout<I>(
    workspace: &Workspace,
    pat: &str,
    arguments: I,
    destination: &Path,
    verification: DownloadVerification,
) -> Result<(), ProviderFailure>
where
    I: IntoIterator<Item = OsString>,
{
    download_to_file_inner(
        workspace,
        pat,
        arguments,
        destination,
        verification,
        #[cfg(test)]
        None,
    )
    .await
}

#[cfg(test)]
pub(super) async fn download_to_file_with_timeout_observed<I>(
    workspace: &Workspace,
    pat: &str,
    arguments: I,
    destination: &Path,
    verification: DownloadVerification,
    copy_complete: oneshot::Sender<()>,
) -> Result<(), ProviderFailure>
where
    I: IntoIterator<Item = OsString>,
{
    download_to_file_inner(
        workspace,
        pat,
        arguments,
        destination,
        verification,
        Some(copy_complete),
    )
    .await
}

async fn download_to_file_inner<I>(
    workspace: &Workspace,
    pat: &str,
    arguments: I,
    destination: &Path,
    verification: DownloadVerification,
    #[cfg(test)] copy_complete: Option<oneshot::Sender<()>>,
) -> Result<(), ProviderFailure>
where
    I: IntoIterator<Item = OsString>,
{
    if verification.expected_size == 0
        || verification.expected_size > verification.max_size
        || !valid_sha256(&verification.expected_sha256)
    {
        return Err(ProviderFailure::ReleaseAssets);
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options
        .open(destination)
        .map_err(|_| ProviderFailure::GhOutput)?;

    let mut command = Command::new(workspace.executable());
    command
        .args(arguments)
        .env_clear()
        .env("GH_TOKEN", pat)
        .env("GH_CONFIG_DIR", workspace.config())
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("PAGER", "cat")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .env("HOME", workspace.root())
        .env("TMPDIR", workspace.root())
        .env("TMP", workspace.root())
        .env("TEMP", workspace.root())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let Ok(mut child) = command.spawn() else {
        let _remove_result = std::fs::remove_file(destination);
        return Err(ProviderFailure::GhUnavailable);
    };
    let stdout = child.stdout.take();
    let destination = destination.to_owned();
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let (ready_tx, ready_rx) = oneshot::channel();
    let (commit_tx, commit_rx) = oneshot::channel();

    // This task owns the writer and child until it commits or cleans up.
    let supervisor = tokio::spawn(supervise_download(DownloadTask {
        child,
        stdout,
        file: tokio::fs::File::from_std(file),
        destination,
        verification,
        cancel: cancel_rx,
        ready: ready_tx,
        commit: commit_rx,
        #[cfg(test)]
        copy_complete,
    }));
    let _cancel = cancel_tx;

    match ready_rx.await {
        Ok(Ok(())) => {
            if commit_tx.send(()).is_err() {
                return Err(ProviderFailure::GhOutput);
            }
        }
        Ok(Err(error)) => {
            let _supervisor_result = supervisor.await;
            return Err(error);
        }
        Err(_) => return Err(ProviderFailure::GhOutput),
    }
    supervisor.await.map_err(|_| ProviderFailure::GhOutput)?
}

async fn supervise_download(task: DownloadTask) -> Result<(), ProviderFailure> {
    let DownloadTask {
        mut child,
        stdout,
        file,
        destination,
        verification,
        cancel,
        ready,
        commit,
        #[cfg(test)]
        copy_complete,
    } = task;
    let Some(stdout) = stdout else {
        transfer::kill_child_and_wait(&mut child).await;
        let error = ProviderFailure::GhUnavailable;
        let _ready_result = ready.send(Err(error));
        if remove_partial_file(&destination).await.is_err() {
            return Err(ProviderFailure::GhOutput);
        }
        return Err(error);
    };
    let result = finish_download(
        child,
        stdout,
        file,
        verification,
        cancel,
        ready,
        commit,
        #[cfg(test)]
        copy_complete,
    )
    .await;
    if result.is_err() && remove_partial_file(&destination).await.is_err() {
        return Err(ProviderFailure::GhOutput);
    }
    result
}

async fn remove_partial_file(destination: &Path) -> std::io::Result<()> {
    tokio::fs::remove_file(destination).await
}

async fn finish_download(
    mut child: Child,
    stdout: ChildStdout,
    file: tokio::fs::File,
    verification: DownloadVerification,
    mut cancel: oneshot::Receiver<()>,
    ready: oneshot::Sender<Result<(), ProviderFailure>>,
    commit: oneshot::Receiver<()>,
    #[cfg(test)] copy_complete: Option<oneshot::Sender<()>>,
) -> Result<(), ProviderFailure> {
    let deadline_at = tokio::time::Instant::now() + verification.timeout;
    let copy = copy_verified_asset(
        stdout,
        file,
        verification.expected_size,
        verification.max_size,
        verification.expected_sha256,
    );
    tokio::pin!(copy);
    let first = {
        let wait = child.wait();
        tokio::pin!(wait);
        let deadline = tokio::time::sleep_until(deadline_at);
        tokio::pin!(deadline);
        tokio::select! {
            biased;
            _ = &mut cancel => FirstCompletion::Cancelled,
            result = &mut copy => FirstCompletion::Copy(result),
            status = &mut wait => FirstCompletion::Child(status),
            () = &mut deadline => FirstCompletion::Timeout,
        }
    };

    let result = complete_transfer(
        first,
        &mut child,
        copy.as_mut(),
        &mut cancel,
        deadline_at,
        #[cfg(test)]
        copy_complete,
    )
    .await;
    if let Err(error) = result {
        let _ready_result = ready.send(Err(error));
        return Err(error);
    }
    if ready.send(Ok(())).is_err() {
        return Err(ProviderFailure::GhOutput);
    }
    tokio::select! {
        biased;
        _ = &mut cancel => Err(ProviderFailure::GhOutput),
        result = commit => result.map_err(|_| ProviderFailure::GhOutput),
    }
}

async fn copy_verified_asset<R>(
    mut reader: R,
    mut file: tokio::fs::File,
    expected_size: u64,
    max_size: u64,
    expected_sha256: String,
) -> Result<(), ProviderFailure>
where
    R: AsyncRead + Unpin,
{
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut chunk = vec![0_u8; DOWNLOAD_CHUNK_BYTES];
    loop {
        let read = reader
            .read(&mut chunk)
            .await
            .map_err(|_| ProviderFailure::GhOutput)?;
        if read == 0 {
            break;
        }
        let read = u64::try_from(read).map_err(|_| ProviderFailure::ReleaseAssets)?;
        let next_total = total
            .checked_add(read)
            .ok_or(ProviderFailure::ReleaseAssets)?;
        if next_total > expected_size || next_total > max_size {
            return Err(ProviderFailure::ReleaseAssets);
        }
        let read = usize::try_from(read).map_err(|_| ProviderFailure::ReleaseAssets)?;
        file.write_all(&chunk[..read])
            .await
            .map_err(|_| ProviderFailure::GhOutput)?;
        hasher.update(&chunk[..read]);
        total = next_total;
    }
    file.flush().await.map_err(|_| ProviderFailure::GhOutput)?;
    if total != expected_size || super::super::hex(&hasher.finalize()) != expected_sha256 {
        return Err(ProviderFailure::ReleaseAssets);
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    super::super::valid_hex(value, 64)
}
