use bollard::models::ContainerCreateBody;
use bollard::query_parameters::CreateContainerOptions;

use crate::error::HostError;
use crate::journal::StorageProvisioningPlan;
use crate::launch::VerifiedWorkerImages;
use crate::launch::bootstrap::BootstrapContainerRole;
use crate::worker::{ResourceBudget, WorkerIdentityNames};

use super::create_body;

#[cfg(test)]
mod tests;

pub(crate) struct PairCreate {
    pub(crate) options: CreateContainerOptions,
    pub(crate) config: ContainerCreateBody,
}

pub(crate) fn create_spec(
    role: BootstrapContainerRole,
    container_name: String,
    network_mode: String,
    names: &WorkerIdentityNames,
    connection_id: &str,
    plan: &StorageProvisioningPlan,
    images: &VerifiedWorkerImages,
    budget: ResourceBudget,
) -> Result<PairCreate, HostError> {
    let image = match role {
        BootstrapContainerRole::Runner => images.runner().runtime_id(),
        BootstrapContainerRole::Dind => images.dind().runtime_id(),
    };
    let body = create_body(
        container_name.clone(),
        image.to_owned(),
        plan,
        names,
        connection_id,
        role,
        network_mode,
        budget,
    )?;
    if body.options.name.as_deref() != Some(container_name.as_str())
        || body.options.platform != "linux/amd64"
    {
        return Err(HostError::Identity);
    }
    Ok(body)
}
