// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;

use crate::docker_client::DOCKER_OPERATION_TIMEOUT;
use crate::error::HostError;
use crate::journal::{Journal, ProbePhase, ProbeRow};

use super::super::inspect;
use super::super::lifecycle::Deadline;
use super::super::projection::ProbeProjection;
use super::verify_bound_engine;

pub(super) async fn abort_if_never_created(
    docker: &Docker,
    journal: &Journal,
    row: &ProbeRow,
    projection: &ProbeProjection,
    deadline: &Deadline,
) -> Result<bool, HostError> {
    if row.container_id.is_some()
        || !matches!(
            row.phase,
            ProbePhase::CreateRequested | ProbePhase::Quarantined
        )
    {
        return Ok(false);
    }
    verify_bound_engine(docker, row, projection, deadline).await?;
    if inspect::by_reference(docker, &projection.name, deadline)
        .await?
        .is_some()
    {
        return Ok(false);
    }
    if deadline.remaining() <= DOCKER_OPERATION_TIMEOUT {
        return Err(HostError::DockerTimeout);
    }
    tokio::time::sleep(DOCKER_OPERATION_TIMEOUT).await;
    verify_bound_engine(docker, row, projection, deadline).await?;
    if inspect::by_reference(docker, &projection.name, deadline)
        .await?
        .is_some()
    {
        return Ok(false);
    }
    deadline
        .within(journal.abort_unattached_probe(
            &row.operation_id,
            &row.engine_id,
            &row.docker_root_digest,
        ))
        .await??;
    Ok(true)
}
