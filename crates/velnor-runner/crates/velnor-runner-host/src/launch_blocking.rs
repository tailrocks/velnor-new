// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::daemon_lock::{EngineLineageGuard, canonical_journal_path};
use crate::docker_client::connect_unix;
use crate::error::HostError;
use crate::journal::Journal;
use crate::launch::{self, LaunchReport};
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

#[derive(Debug, thiserror::Error)]
pub enum ListenFault {
    #[error(transparent)]
    Host(#[from] HostError),

    #[error(transparent)]
    Ensure(#[from] EnsureError),
}

pub fn launch_blocking(
    pat: &str,
    owner: &str,
    repo: &str,
    endpoint: &str,
    journal_path: &Path,
    max_jobs: u32,
    resource_budget: ResourceBudget,
) -> Result<LaunchReport, ListenFault> {
    let _guard = launch::install_job_capacity(max_jobs);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)?;
    runtime.block_on(drive(
        pat,
        owner,
        repo,
        endpoint,
        journal_path,
        resource_budget,
    ))
}

async fn drive(
    pat: &str,
    owner: &str,
    repo: &str,
    endpoint: &str,
    journal_path: &Path,
    resource_budget: ResourceBudget,
) -> Result<LaunchReport, ListenFault> {
    let docker = connect_unix(endpoint)?;
    let engine_id = docker
        .info()
        .await
        .map_err(|_| HostError::Docker)?
        .id
        .ok_or(HostError::Docker)?;
    let journal_path = canonical_journal_path(journal_path)?;
    let lineage_guard = EngineLineageGuard::acquire(&engine_id)?;
    let journal = Journal::open(&journal_path).await?;
    journal.bind_engine(&engine_id).await?;
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    lineage_guard.verify_lineage(&journal_path, &instance_id, revision)?;
    journal.attach_lineage_guard(lineage_guard.clone())?;
    let launched = launch::launch_once(pat, owner, repo, &docker, &journal, resource_budget).await;
    let revision = journal.revision().await?;
    lineage_guard.advance_revision(revision)?;
    Ok(launched?)
}
