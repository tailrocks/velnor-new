//! Size- and digest-bounded release asset download from the GitHub CLI.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

use super::super::ProviderFailure;
use super::Workspace;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(15);
const DOWNLOAD_CHUNK_BYTES: usize = 64 * 1024;

enum FirstCompletion {
    Copy(Result<Result<(), ProviderFailure>, tokio::task::JoinError>),
    Child(std::io::Result<std::process::ExitStatus>),
    Timeout,
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
    if expected_size == 0 || expected_size > max_size || !valid_sha256(expected_sha256) {
        return Err(ProviderFailure::ReleaseAssets);
    }

    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options
        .open(destination)
        .await
        .map_err(|_| ProviderFailure::GhOutput)?;

    let result = download_child_to_file(
        workspace,
        pat,
        arguments,
        file,
        expected_size,
        max_size,
        expected_sha256,
    )
    .await;
    if result.is_err() && tokio::fs::remove_file(destination).await.is_err() {
        return Err(ProviderFailure::GhOutput);
    }
    result
}

async fn download_child_to_file<I>(
    workspace: &Workspace,
    pat: &str,
    arguments: I,
    file: tokio::fs::File,
    expected_size: u64,
    max_size: u64,
    expected_sha256: &str,
) -> Result<(), ProviderFailure>
where
    I: IntoIterator<Item = OsString>,
{
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
    let mut child = command
        .spawn()
        .map_err(|_| ProviderFailure::GhUnavailable)?;
    let Some(stdout) = child.stdout.take() else {
        let _kill_result = child.start_kill();
        let _wait_result = child.wait().await;
        return Err(ProviderFailure::GhUnavailable);
    };
    let copy = tokio::spawn(copy_verified_asset(
        stdout,
        file,
        expected_size,
        max_size,
        expected_sha256.to_owned(),
    ));
    finish_download(child, copy).await
}

async fn finish_download(
    mut child: Child,
    mut copy: tokio::task::JoinHandle<Result<(), ProviderFailure>>,
) -> Result<(), ProviderFailure> {
    let deadline_at = tokio::time::Instant::now() + DOWNLOAD_TIMEOUT;
    let first = {
        let wait = child.wait();
        tokio::pin!(wait);
        let deadline = tokio::time::sleep_until(deadline_at);
        tokio::pin!(deadline);
        tokio::select! {
            result = &mut copy => FirstCompletion::Copy(result),
            status = &mut wait => FirstCompletion::Child(status),
            () = &mut deadline => FirstCompletion::Timeout,
        }
    };

    match first {
        FirstCompletion::Copy(Ok(Ok(()))) => {
            match tokio::time::timeout_at(deadline_at, child.wait()).await {
                Ok(Ok(status)) if status.success() => Ok(()),
                Ok(Ok(_)) => Err(ProviderFailure::GhCommand),
                Ok(Err(_)) => Err(ProviderFailure::GhUnavailable),
                Err(_) => {
                    kill_child_and_wait(&mut child).await;
                    Err(ProviderFailure::GhTimeout)
                }
            }
        }
        FirstCompletion::Child(Ok(status)) => {
            match tokio::time::timeout_at(deadline_at, &mut copy).await {
                Ok(Ok(Ok(()))) if status.success() => Ok(()),
                Ok(Ok(Ok(()))) => Err(ProviderFailure::GhCommand),
                Ok(Ok(Err(error))) => Err(error),
                Ok(Err(_)) => Err(ProviderFailure::GhOutput),
                Err(_) => {
                    stop_child(&mut child, &mut copy).await;
                    Err(ProviderFailure::GhTimeout)
                }
            }
        }
        FirstCompletion::Copy(Ok(Err(error))) => {
            kill_child_and_wait(&mut child).await;
            Err(error)
        }
        FirstCompletion::Copy(Err(_)) => {
            kill_child_and_wait(&mut child).await;
            Err(ProviderFailure::GhUnavailable)
        }
        FirstCompletion::Child(Err(_)) => {
            stop_child(&mut child, &mut copy).await;
            Err(ProviderFailure::GhUnavailable)
        }
        FirstCompletion::Timeout => {
            stop_child(&mut child, &mut copy).await;
            Err(ProviderFailure::GhTimeout)
        }
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

async fn stop_child(
    child: &mut Child,
    copy: &mut tokio::task::JoinHandle<Result<(), ProviderFailure>>,
) {
    copy.abort();
    let _copy_result = copy.await;
    kill_child_and_wait(child).await;
}

async fn kill_child_and_wait(child: &mut Child) {
    let _kill_result = child.start_kill();
    let _wait_result = child.wait().await;
}

fn valid_sha256(value: &str) -> bool {
    super::super::valid_hex(value, 64)
}
