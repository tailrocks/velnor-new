// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::ContainerCreateBody;
use bollard::query_parameters::CreateContainerOptions;

use crate::config::RunnerPlatform;
use crate::error::HostError;
use crate::journal::StorageProvisioningPlan;
use crate::launch::VerifiedWorkerImages;
use crate::launch::bootstrap::BootstrapContainerRole;
use crate::worker::{ResourceBudget, WorkerIdentityNames};

use super::create_body;

#[derive(Debug)]
pub(crate) struct PairCreate {
    pub(crate) options: CreateContainerOptions,
    pub(crate) config: ContainerCreateBody,
}

pub(crate) struct PairSpecContext<'a> {
    pub(crate) plan: &'a StorageProvisioningPlan,
    pub(crate) names: &'a WorkerIdentityNames,
    pub(crate) connection_id: &'a str,
    pub(crate) budget: ResourceBudget,
}

pub(crate) fn create_spec(
    role: BootstrapContainerRole,
    container_name: &str,
    network_mode: String,
    images: &VerifiedWorkerImages,
    context: &PairSpecContext<'_>,
) -> Result<PairCreate, HostError> {
    let image = match role {
        BootstrapContainerRole::Runner => images.runner().runtime_id(),
        BootstrapContainerRole::Dind => images.dind().runtime_id(),
    };
    let platform = RunnerPlatform::parse(images.platform()).ok_or(HostError::Identity)?;
    let body = create_body(
        container_name.to_owned(),
        image.to_owned(),
        role,
        network_mode,
        platform,
        context,
    )?;
    if body.options.name.as_deref() != Some(container_name)
        || body.options.platform != images.platform()
    {
        return Err(HostError::Identity);
    }
    Ok(body)
}
