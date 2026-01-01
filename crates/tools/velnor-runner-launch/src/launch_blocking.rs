//! Current-thread `launch_once`. The capacity override stays on this thread.

use std::path::Path;

use crate::launch::{self, LaunchReport};
use velnor_runner_host::HostError;
use velnor_runner_host::docker_client::connect_unix;
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_journal::journal::Journal;

/// `launch_once` failed, or the local socket or journal did.
#[derive(Debug, thiserror::Error)]
pub enum ListenFault {
    /// Docker, journal, or runtime setup failed. No token.
    #[error(transparent)]
    Host(#[from] HostError),
    /// Registration, acquire, or session failed. No token.
    #[error(transparent)]
    Ensure(#[from] EnsureError),
}

/// Open one session on a current-thread runtime.
///
/// `max_jobs` is installed on this thread before the runtime polls, so
/// `job_capacity` sees the host file instead of only `VELNOR_MAX_JOBS`.
///
/// # Errors
///
/// Returns [`ListenFault`] when the runtime, socket, journal, or launch fails.
/// The display text is the inner error and does not include `pat`.
pub fn launch_blocking(
    pat: &str,
    owner: &str,
    repo: &str,
    endpoint: &str,
    journal_path: &Path,
    max_jobs: u32,
) -> Result<LaunchReport, ListenFault> {
    let _guard = launch::install_job_capacity(max_jobs);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)?;
    runtime.block_on(drive(pat, owner, repo, endpoint, journal_path))
}

async fn drive(
    pat: &str,
    owner: &str,
    repo: &str,
    endpoint: &str,
    journal_path: &Path,
) -> Result<LaunchReport, ListenFault> {
    let docker = connect_unix(endpoint)?;
    let journal = Journal::open(journal_path).await?;
    Ok(launch::launch_once(pat, owner, repo, &docker, &journal).await?)
}
