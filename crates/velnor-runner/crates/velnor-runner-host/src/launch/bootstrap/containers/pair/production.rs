use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::container::AttachContainerResults;
use bollard::query_parameters::{AttachContainerOptionsBuilder, StartContainerOptions};

use crate::error::HostError;
use crate::journal::{
    ConnectionJournal, RecordedStorageNetwork, VerifiedMetadataInitializationHandoff,
};
use crate::launch::{SelectedEngineContext, VerifiedWorkerImages};
use crate::worker::storage_initialization::PendingStorageInitialization;

use super::super::super::adapter::PreparedAcquiredBootstrap;
use super::super::{
    BootstrapContainerRole, BootstrapStartReceipt, VerifiedBootstrapContainerCreated,
    VerifiedBootstrapMountHandoff,
};
use super::spec::create_spec;
use super::{PairCreate, PairStartFailure, StartedBootstrapPair, fixed_mounts};
mod inputs;
use inputs::validate_pair_inputs;
mod mounts;
use mounts::{create_handoff, inspect_volume_mounts};

const PAIR_TIMEOUT: Duration = Duration::from_secs(90);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Create and start one journal-owned `DinD` and Runner pair.
pub(crate) async fn start_bootstrap_pair(
    prepared: &PreparedAcquiredBootstrap,
    network: &RecordedStorageNetwork,
    metadata: &VerifiedMetadataInitializationHandoff,
    pending_initialization: &PendingStorageInitialization,
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
) -> Result<StartedBootstrapPair, PairStartFailure> {
    let (reservation_id, names) = validate_pair_inputs(
        prepared,
        network,
        metadata,
        pending_initialization,
        journal,
        engine,
        images,
    )
    .map_err(PairStartFailure::before_attach)?;
    let runner_mounts = fixed_mounts(&names, BootstrapContainerRole::Runner)
        .map_err(PairStartFailure::before_attach)?;
    let dind_mounts = fixed_mounts(&names, BootstrapContainerRole::Dind)
        .map_err(PairStartFailure::before_attach)?;
    let deadline = Instant::now() + PAIR_TIMEOUT;
    let (dind_id, runner_id) = create_pair_containers(
        prepared, network, docker, journal, engine, images, &names, deadline,
    )
    .await
    .map_err(PairStartFailure::before_attach)?;
    let start_claim = request(deadline, journal.claim_bootstrap_start(reservation_id))
        .await
        .map_err(PairStartFailure::before_attach)?
        .map_err(PairStartFailure::before_attach)?
        .ok_or_else(|| PairStartFailure::before_attach(HostError::Journal))?;
    if start_claim.runner_container_id() != runner_id
        || start_claim.dind_container_id() != dind_id
        || start_claim.engine_id() != engine.engine_id()
        || start_claim.docker_root_digest() != engine.docker_root_digest()
    {
        mark_start_uncertain(journal, &start_claim).await;
        return Err(PairStartFailure::before_attach(HostError::Identity));
    }
    let attached = match attach_runner(docker, &runner_id, deadline).await {
        Ok(attached) => attached,
        Err(error) => {
            mark_start_uncertain(journal, &start_claim).await;
            return Err(PairStartFailure::before_attach(error));
        }
    };
    start_attached_pair(
        docker,
        journal,
        engine,
        images,
        start_claim,
        attached,
        metadata,
        &runner_mounts,
        &dind_mounts,
        deadline,
    )
    .await
}

async fn create_pair_containers(
    prepared: &PreparedAcquiredBootstrap,
    network: &RecordedStorageNetwork,
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    names: &crate::worker::WorkerIdentityNames,
    deadline: Instant,
) -> Result<(String, String), HostError> {
    let dind_id = create_one(
        BootstrapContainerRole::Dind,
        network.network_name(),
        prepared,
        docker,
        journal,
        engine,
        images,
        names,
        deadline,
    )
    .await?;
    let runner_network = format!("container:{dind_id}");
    let runner_id = create_one(
        BootstrapContainerRole::Runner,
        &runner_network,
        prepared,
        docker,
        journal,
        engine,
        images,
        names,
        deadline,
    )
    .await?;
    Ok((dind_id, runner_id))
}

pub(super) async fn start_attached_pair(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: crate::journal::BootstrapStartClaim,
    mut attached: AttachContainerResults,
    metadata: &VerifiedMetadataInitializationHandoff,
    runner_mounts: &[bollard::models::Mount],
    dind_mounts: &[bollard::models::Mount],
    deadline: Instant,
) -> Result<StartedBootstrapPair, PairStartFailure> {
    match start_pair(
        docker,
        journal,
        engine,
        images,
        &claim,
        deadline,
        &mut attached,
        metadata,
        runner_mounts,
        dind_mounts,
    )
    .await
    {
        Ok((receipt, mount_handoff)) => Ok(StartedBootstrapPair {
            receipt,
            mount_handoff,
            stdin: attached.input,
            output: Some(attached.output),
            stdin_open: true,
            jit_withheld: true,
            delivery_attempted: false,
            delivery_succeeded: false,
        }),
        Err(error) => {
            mark_start_uncertain(journal, &claim).await;
            Err(PairStartFailure::after_attach(error, claim, attached))
        }
    }
}

async fn mark_start_uncertain(
    journal: &ConnectionJournal,
    claim: &crate::journal::BootstrapStartClaim,
) {
    let _marked = tokio::time::timeout(
        REQUEST_TIMEOUT,
        journal.mark_bootstrap_start_uncertain(claim),
    )
    .await;
}

async fn create_one(
    role: BootstrapContainerRole,
    network_mode: &str,
    prepared: &PreparedAcquiredBootstrap,
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    names: &crate::worker::WorkerIdentityNames,
    deadline: Instant,
) -> Result<String, HostError> {
    let Some(claim) = request(
        deadline,
        journal.claim_bootstrap_container_create(prepared.acquired().reservation_id(), role),
    )
    .await??
    else {
        return Err(HostError::Journal);
    };
    let result = async {
        if claim.operation_id() != prepared.acquired().intent_id()
            || claim.engine_id() != engine.engine_id()
            || claim.docker_root_digest() != engine.docker_root_digest()
        {
            return Err(HostError::Identity);
        }
        let spec = create_spec(
            role,
            claim.container_name().to_owned(),
            network_mode.to_owned(),
            names,
            journal.connection_id().as_str(),
            claim.storage_plan(),
            images,
            prepared.acquired().resource_budget(),
        )?;
        create_claimed(docker, journal, engine, images, &claim, &spec, deadline).await
    }
    .await;
    if result.is_err() {
        let _marked = tokio::time::timeout(
            REQUEST_TIMEOUT,
            journal.mark_bootstrap_container_create_uncertain(&claim),
        )
        .await;
    }
    result
}

async fn create_claimed(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &crate::journal::BootstrapContainerCreateClaim,
    spec: &PairCreate,
    deadline: Instant,
) -> Result<String, HostError> {
    request(deadline, images.verify_for_engine(docker, engine)).await??;
    if inspect(docker, claim.container_name(), deadline)
        .await?
        .is_some()
    {
        return Err(HostError::Ownership);
    }
    request(deadline, images.verify_for_engine(docker, engine)).await??;
    let image_reference = spec.config.image.as_deref().ok_or(HostError::Identity)?;
    let image = request(deadline, docker.inspect_image(image_reference))
        .await?
        .map_err(|_| HostError::Docker)?;
    let expected_config = match claim.role() {
        BootstrapContainerRole::Runner => images.runner().config_digest(),
        BootstrapContainerRole::Dind => images.dind().config_digest(),
    };
    if image.id.as_deref() != Some(expected_config) {
        return Err(HostError::Ownership);
    }
    let image_config = image.config.ok_or(HostError::Identity)?;
    let created = request(
        deadline,
        docker.create_container(Some(spec.options.clone()), spec.config.clone()),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    if !valid_resource_id(&created.id) {
        return Err(HostError::Docker);
    }
    request(deadline, images.verify_for_engine(docker, engine)).await??;
    let inspected = inspect(docker, &created.id, deadline)
        .await?
        .ok_or(HostError::Docker)?;
    verify_created(&inspected, &created.id, claim, spec, &image_config)?;
    let proof = VerifiedBootstrapContainerCreated {
        reservation_id: claim.reservation_id(),
        operation_id: claim.operation_id(),
        engine_id: claim.engine_id().to_owned(),
        docker_root_digest: claim.docker_root_digest().to_owned(),
        role: claim.role(),
        container_name: claim.container_name().to_owned(),
        container_id: created.id.clone(),
    };
    request(
        deadline,
        journal.record_bootstrap_container_created(claim, &proof),
    )
    .await??;
    Ok(created.id)
}

async fn attach_runner(
    docker: &Docker,
    runner_id: &str,
    deadline: Instant,
) -> Result<AttachContainerResults, HostError> {
    request(
        deadline,
        docker.attach_container(
            runner_id,
            Some(
                AttachContainerOptionsBuilder::default()
                    .stdin(true)
                    .stdout(false)
                    .stderr(false)
                    .stream(true)
                    .logs(false)
                    .build(),
            ),
        ),
    )
    .await?
    .map_err(|_| HostError::Docker)
}

async fn start_pair(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &crate::journal::BootstrapStartClaim,
    deadline: Instant,
    _stdin: &mut AttachContainerResults,
    metadata: &VerifiedMetadataInitializationHandoff,
    runner_mounts: &[bollard::models::Mount],
    dind_mounts: &[bollard::models::Mount],
) -> Result<(BootstrapStartReceipt, VerifiedBootstrapMountHandoff), HostError> {
    let mut digests = Vec::with_capacity(2);
    for (role, container_id, mounts) in [
        ("dind", claim.dind_container_id(), dind_mounts),
        ("runner", claim.runner_container_id(), runner_mounts),
    ] {
        request(deadline, images.verify_for_engine(docker, engine)).await??;
        request(
            deadline,
            docker.start_container(container_id, None::<StartContainerOptions>),
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        let inspected = inspect(docker, container_id, deadline)
            .await?
            .ok_or(HostError::Docker)?;
        if inspected.id.as_deref() != Some(container_id)
            || inspected.state.as_ref().and_then(|state| state.running) != Some(true)
        {
            return Err(HostError::Ownership);
        }
        digests.push(inspect_volume_mounts(&inspected, mounts, role)?);
    }
    let proof = BootstrapStartReceipt {
        reservation_id: claim.reservation_id(),
        operation_id: claim.operation_id(),
        engine_id: claim.engine_id().to_owned(),
        docker_root_digest: claim.docker_root_digest().to_owned(),
        runner_container_id: claim.runner_container_id().to_owned(),
        dind_container_id: claim.dind_container_id().to_owned(),
        runner_stdin_open: true,
        jit_withheld: true,
    };
    request(deadline, journal.record_bootstrap_started(claim, &proof)).await??;
    let runner_digest = digests.get(1).ok_or(HostError::Identity)?;
    let dind_digest = digests.first().ok_or(HostError::Identity)?;
    let handoff = create_handoff(metadata, claim, runner_digest, dind_digest)?;
    Ok((proof, handoff))
}

mod verify;
use verify::{inspect, request, valid_resource_id, verify_created};
