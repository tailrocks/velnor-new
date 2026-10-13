//! Verify the selected engine before any initial cleanup request.

use std::time::Duration;

use bollard::Docker;

use crate::docker_client::DOCKER_OPERATION_TIMEOUT;
use crate::journal::Journal;
use crate::scale_set::EnsureError;
use crate::stage::PairEngine;
use crate::worker::ResourceBudget;

use super::resource_capacity::{self, Discovery, JobCapacity};
use super::slot;

pub(super) async fn run<E: PairEngine + ?Sized>(
    docker: &Docker,
    cleanup_engine: &E,
    journal: &Journal,
    budget: ResourceBudget,
    ceiling: u32,
) -> Result<JobCapacity, EnsureError> {
    run_after(
        docker,
        cleanup_engine,
        journal,
        budget,
        ceiling,
        DOCKER_OPERATION_TIMEOUT,
    )
    .await
}

async fn run_after<E: PairEngine + ?Sized>(
    docker: &Docker,
    cleanup_engine: &E,
    journal: &Journal,
    budget: ResourceBudget,
    ceiling: u32,
    timeout: Duration,
) -> Result<JobCapacity, EnsureError> {
    resource_capacity::verify_engine_binding_after(docker, journal, timeout).await?;
    slot::release_exited(journal, cleanup_engine).await?;
    match resource_capacity::discover_after(docker, journal, budget, ceiling, timeout).await {
        Discovery::Available(capacity) => Ok(capacity),
        Discovery::Unavailable => Ok(JobCapacity::denied()),
        Discovery::Untrusted(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "preflight_tests.rs"]
mod tests;
