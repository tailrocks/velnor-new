use std::time::Instant;

use super::CLEANUP_TIMEOUT;

use bollard::{
    Docker, errors::Error as DockerError, query_parameters::RemoveContainerOptionsBuilder,
};

use crate::{
    error::HostError,
    journal::{
        ConnectionJournal, PreJitBootstrapContainer, PreJitCleanupClaim, PreJitCleanupInventory,
        PreJitCreateSpec,
    },
    launch::{SelectedEngineContext, VerifiedWorkerImages, bootstrap::containers},
    worker::{WorkerContainerRole, WorkerIdentityNames},
};

use super::super::{PreJitCleanupPhase, PreJitCleanupPhaseProof};
use super::{inspect_container, request, verify_selection};

mod evidence;
use evidence::{BootstrapPhaseEvidence, ContainerObservation};

pub(super) async fn remove_bootstrap_phase(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    inventory: &PreJitCleanupInventory,
) -> Result<(), HostError> {
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    verify_selection(docker, engine, images, claim, deadline).await?;
    let names = WorkerIdentityNames::from_private_volume(
        inventory
            .initializer()
            .home_volume()
            .strip_suffix("-home")
            .ok_or(HostError::Ownership)?,
    )?;
    let budget = inventory
        .create_specs()
        .first()
        .ok_or(HostError::Ownership)?
        .budget()
        .validate()?;
    let mut observations = Vec::new();
    for role in [BootstrapRole::Dind, BootstrapRole::Runner] {
        if let Some(observation) = remove_one_bootstrap_container(
            docker, engine, images, claim, inventory, &names, budget, role, deadline,
        )
        .await?
        {
            observations.push(observation);
        }
    }
    let selected = verify_selection(docker, engine, images, claim, deadline).await?;
    let evidence = serde_json::to_vec(&BootstrapPhaseEvidence {
        engine_id: selected.engine_id,
        docker_root_digest: selected.docker_root_digest,
        platform: selected.platform,
        daemon_architecture: selected.daemon_architecture,
        containers: observations,
    })
    .map_err(|_| HostError::Identity)?;
    let proof = PreJitCleanupPhaseProof::after_observed_phase(
        claim,
        PreJitCleanupPhase::Claimed,
        PreJitCleanupPhase::BootstrapAbsent,
        &evidence,
    )?;
    journal.record_pre_jit_cleanup_phase(claim, &proof).await
}

async fn remove_one_bootstrap_container(
    docker: &Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    inventory: &PreJitCleanupInventory,
    names: &WorkerIdentityNames,
    budget: crate::launch::ResourceBudget,
    role: BootstrapRole,
    deadline: Instant,
) -> Result<Option<ContainerObservation>, HostError> {
    let Some((row, receipt)) = row_and_spec(inventory, role)? else {
        return Ok(None);
    };
    let expected_name = names.container_name(role.worker_role());
    let expected_spec = expected_bootstrap_spec(
        role,
        row,
        receipt,
        images,
        claim,
        inventory,
        names,
        budget,
        &expected_name,
    )?;
    let Some(inspected) =
        inspect_bootstrap_container(docker, &expected_name, row.id(), deadline).await?
    else {
        return Ok(Some(ContainerObservation::new(
            row.role(),
            &expected_name,
            row.id(),
            "already_absent",
            receipt.spec_sha256(),
            receipt.image_runtime_id(),
            None,
        )));
    };
    let actual_id = inspected
        .id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or(HostError::Ownership)?;
    if row.id().is_some_and(|saved| saved != actual_id)
        || inspected
            .state
            .as_ref()
            .and_then(|state| state.running)
            .unwrap_or(true)
    {
        return Err(HostError::Ownership);
    }
    verify_and_remove_bootstrap_container(
        docker,
        engine,
        images,
        claim,
        role,
        &expected_name,
        actual_id,
        &expected_spec,
        &inspected,
        deadline,
    )
    .await?;
    Ok(Some(ContainerObservation::new(
        row.role(),
        &expected_name,
        Some(actual_id),
        "removed_and_absent",
        receipt.spec_sha256(),
        receipt.image_runtime_id(),
        inspected
            .config
            .as_ref()
            .and_then(|config| config.labels.clone()),
    )))
}

async fn inspect_bootstrap_container(
    docker: &Docker,
    expected_name: &str,
    saved_id: Option<&str>,
    deadline: Instant,
) -> Result<Option<bollard::models::ContainerInspectResponse>, HostError> {
    let inspected = inspect_container(docker, expected_name, deadline).await?;
    if inspected.is_some() {
        return Ok(inspected);
    }
    let saved_id_is_present = if let Some(saved_id) = saved_id {
        inspect_container(docker, saved_id, deadline)
            .await?
            .is_some()
    } else {
        false
    };
    require_saved_id_absent(saved_id, saved_id_is_present)?;
    Ok(None)
}

fn expected_bootstrap_spec(
    role: BootstrapRole,
    row: &PreJitBootstrapContainer,
    receipt: &PreJitCreateSpec,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    inventory: &PreJitCleanupInventory,
    names: &WorkerIdentityNames,
    budget: crate::launch::ResourceBudget,
    expected_name: &str,
) -> Result<containers::PairCreate, HostError> {
    if row.name() != expected_name
        || receipt.name() != expected_name
        || receipt.image_runtime_id() != image_for(role, images).runtime_id()
        || receipt.image_source_sha() != images.source_sha()
        || receipt.budget() != inventory.create_specs()[0].budget()
    {
        return Err(HostError::Ownership);
    }
    let dind_id = inventory
        .bootstrap_containers()
        .iter()
        .find(|container| container.role() == "dind")
        .and_then(PreJitBootstrapContainer::id);
    if role == BootstrapRole::Runner && dind_id.is_none() {
        return Err(HostError::Ownership);
    }
    let network_mode = match role {
        BootstrapRole::Dind => inventory.network().name().to_owned(),
        BootstrapRole::Runner => format!("container:{}", dind_id.ok_or(HostError::Ownership)?),
    };
    let spec_context = containers::PairSpecContext {
        plan: inventory.storage_plan(),
        names,
        connection_id: claim.connection_id(),
        budget,
    };
    let spec = containers::create_spec(
        role.container_role(),
        expected_name,
        network_mode,
        images,
        &spec_context,
    )?;
    if containers::create_spec_sha256(&spec)? != receipt.spec_sha256() {
        return Err(HostError::Ownership);
    }
    Ok(spec)
}

async fn verify_and_remove_bootstrap_container(
    docker: &Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    role: BootstrapRole,
    expected_name: &str,
    actual_id: &str,
    spec: &containers::PairCreate,
    inspected: &bollard::models::ContainerInspectResponse,
    deadline: Instant,
) -> Result<(), HostError> {
    let image = image_for(role, images);
    verify_selection(docker, engine, images, claim, deadline).await?;
    let image_inspect = request(deadline, docker.inspect_image(image.runtime_id()))
        .await?
        .map_err(|_| HostError::Docker)?;
    if image_inspect.id.as_deref() != Some(image.runtime_id())
        || image_inspect.os.as_deref() != Some("linux")
        || image_inspect.architecture.as_deref() != Some("amd64")
    {
        return Err(HostError::Ownership);
    }
    let image_config = image_inspect.config.as_ref().ok_or(HostError::Ownership)?;
    containers::verify_pre_jit_created(
        inspected,
        actual_id,
        &format!("/{expected_name}"),
        spec,
        image_config,
    )?;
    verify_selection(docker, engine, images, claim, deadline).await?;
    let options = RemoveContainerOptionsBuilder::new().force(false).build();
    match request(deadline, docker.remove_container(actual_id, Some(options))).await? {
        Ok(()) => {}
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => return Err(HostError::Ownership),
        Err(_) => return Err(HostError::Docker),
    }
    verify_selection(docker, engine, images, claim, deadline).await?;
    if inspect_container(docker, actual_id, deadline)
        .await?
        .is_some()
        || inspect_container(docker, expected_name, deadline)
            .await?
            .is_some()
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BootstrapRole {
    Runner,
    Dind,
}

impl BootstrapRole {
    const fn row_role(self) -> &'static str {
        match self {
            Self::Runner => "runner",
            Self::Dind => "dind",
        }
    }
    const fn worker_role(self) -> WorkerContainerRole {
        match self {
            Self::Runner => WorkerContainerRole::Runner,
            Self::Dind => WorkerContainerRole::Dind,
        }
    }
    const fn container_role(self) -> containers::BootstrapContainerRole {
        match self {
            Self::Runner => containers::BootstrapContainerRole::Runner,
            Self::Dind => containers::BootstrapContainerRole::Dind,
        }
    }
}

fn row_and_spec(
    inventory: &PreJitCleanupInventory,
    role: BootstrapRole,
) -> Result<Option<(&PreJitBootstrapContainer, &PreJitCreateSpec)>, HostError> {
    let rows = inventory
        .bootstrap_containers()
        .iter()
        .filter(|row| row.role() == role.row_role())
        .collect::<Vec<_>>();
    let specs = inventory
        .create_specs()
        .iter()
        .filter(|spec| spec.role() == role.row_role())
        .collect::<Vec<_>>();
    if rows.len() != 1 || specs.len() > 1 {
        return Err(HostError::Ownership);
    }
    match specs.first() {
        Some(spec) => {
            if rows[0].create_state() == "unclaimed" {
                return Err(HostError::Ownership);
            }
            Ok(Some((rows[0], *spec)))
        }
        None if rows[0].create_state() == "unclaimed" => Ok(None),
        None => Err(HostError::Ownership),
    }
}

fn image_for(
    role: BootstrapRole,
    images: &VerifiedWorkerImages,
) -> &crate::launch::VerifiedImageIdentity {
    match role {
        BootstrapRole::Runner => images.runner(),
        BootstrapRole::Dind => images.dind(),
    }
}

fn require_saved_id_absent(saved_id: Option<&str>, id_is_present: bool) -> Result<(), HostError> {
    if saved_id.is_some() && id_is_present {
        return Err(HostError::Ownership);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::require_saved_id_absent;
    use crate::error::HostError;

    #[test]
    fn saved_container_id_blocks_absence_when_name_lookup_misses() {
        assert!(matches!(
            require_saved_id_absent(Some("saved-container-id"), true),
            Err(HostError::Ownership)
        ));
        assert!(require_saved_id_absent(Some("saved-container-id"), false).is_ok());
        assert!(require_saved_id_absent(None, false).is_ok());
    }
}
