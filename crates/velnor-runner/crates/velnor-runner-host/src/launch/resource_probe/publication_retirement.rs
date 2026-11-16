// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use bollard::Docker;

use crate::error::HostError;
use crate::journal::{ConnectionJournal, HostHelperRole, Journal, ProbeRow, ProbeWork};
use crate::launch::SelectedEngineContext;

use super::lifecycle::{Deadline, bind_info, info};
use super::{readiness, recover};

pub(crate) struct OwnedPublication {
    operation_id: String,
    operation_name: String,
    container_id: Option<String>,
}

impl OwnedPublication {
    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) fn operation_name(&self) -> &str {
        &self.operation_name
    }

    pub(crate) fn container_id(&self) -> Option<&str> {
        self.container_id.as_deref()
    }
}

pub(crate) async fn owned_publication(
    journal: &Journal,
    scope: &ConnectionJournal,
    reservation_id: i64,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<Option<OwnedPublication>, HostError> {
    let Some(row) = journal.active_probe().await? else {
        return Ok(None);
    };
    if !is_publication_row(&row) {
        return Ok(None);
    }
    let Some(record) = journal.execution_publication_recovery(&row.operation_id).await? else {
        return Ok(None);
    };
    if record.operation_id != row.operation_id
        || record.instance_id != row.instance_id
        || record.reservation_id != reservation_id
        || record.connection_id != scope.connection_id().as_str()
        || record.engine_id != engine_id
        || record.docker_root_digest != docker_root_digest
        || row.engine_id != engine_id
        || row.docker_root_digest != docker_root_digest
    {
        return Ok(None);
    }
    Ok(Some(OwnedPublication {
        operation_id: row.operation_id,
        operation_name: row.operation_name,
        container_id: row.container_id,
    }))
}

fn is_publication_row(row: &ProbeRow) -> bool {
    row.helper_role() == HostHelperRole::ExecutionPublication
        && matches!(row.work(), ProbeWork::ExecutionPublication)
        && row.operation_name == format!("velnor-execution-publication-{}", row.operation_id)
}

pub(crate) async fn retire_owned_publication(
    docker: &Docker,
    journal: &Journal,
    selected: &SelectedEngineContext,
    publication: &OwnedPublication,
    deadline: Instant,
) -> Result<(), HostError> {
    let _operation = readiness::lock_operation(journal).await?;
    let request_deadline = Deadline::until(deadline);
    let before = info(docker, &request_deadline).await?;
    let (engine_id, root, _, _) = bind_info(&before, journal, &request_deadline).await?;
    if selected.engine_id() != engine_id || selected.docker_root_digest() != root.digest() {
        return Err(HostError::Identity);
    }
    let Some(active) = request_deadline.within(journal.active_probe()).await?? else {
        return Ok(());
    };
    if active.operation_id != publication.operation_id || !is_publication_row(&active) {
        return Err(HostError::Ownership);
    }
    Box::pin(recover::active(
        docker,
        journal,
        &before,
        &root,
        &request_deadline,
        active,
    ))
    .await
}
