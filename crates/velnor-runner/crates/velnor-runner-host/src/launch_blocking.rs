//! Current-thread `launch_once`. The capacity override stays on this thread.

use std::path::Path;

use crate::config::HostConfig;
use crate::daemon_lock::{EngineLineageGuard, canonical_journal_path};
use crate::docker_client::connect_unix;
use crate::error::HostError;
use crate::journal::Journal;
use crate::launch::{self, LaunchReport};
use crate::scale_set::EnsureError;

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
/// The parsed host file supplies both admission capacity and required worker
/// resource budgets to the launch path.
///
/// # Errors
///
/// Returns [`ListenFault`] when the runtime, socket, journal, or launch fails.
/// The display text is the inner error and does not include `pat`.
pub fn launch_blocking(
    pat: &str,
    owner: &str,
    repo: &str,
    config: &HostConfig,
    journal_path: &Path,
) -> Result<LaunchReport, ListenFault> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)?;
    runtime.block_on(drive(pat, owner, repo, config, journal_path))
}

async fn drive(
    pat: &str,
    owner: &str,
    repo: &str,
    config: &HostConfig,
    journal_path: &Path,
) -> Result<LaunchReport, ListenFault> {
    let docker = connect_unix(&config.docker.endpoint)?;
    let engine_id = docker
        .info()
        .await
        .map_err(|_| HostError::Docker)?
        .id
        .ok_or(HostError::Docker)?;
    let journal_path = canonical_journal_path(journal_path)?;
    let lineage_guard = EngineLineageGuard::acquire(&engine_id)?;
    let journal = Journal::open(&journal_path).await?;
    journal
        .establish_engine_lineage(&engine_id, lineage_guard.clone())
        .await?;
    let launched = launch::launch_once(pat, owner, repo, &docker, &journal, config).await;
    let revision = journal.revision().await?;
    lineage_guard.advance_revision(revision)?;
    Ok(launched?)
}
