use std::time::{Duration, Instant};

use backing::{apply_backing_capacity, root_backing_mappings, validate_owned_sample};
use bollard::Docker;
use capacity::{instance_binding_digest, observed_cgroup, observed_roles};
use worker::{probe_attachment, validate_guest_identity, validate_plan, worker_attachments};

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal, StorageObserver},
    launch::{
        SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{
            BootstrapContainerRole, PreparedAcquiredBootstrap, StartedBootstrapPair,
            VerifiedStorageResourceBinding,
        },
        resource_probe::{ImageProvider, OwnedCgroupSubjects, OwnedContainerSubjects},
    },
    worker::storage_initialization::VerifiedStorageInitializationReady,
};

const STORAGE_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(45);
#[path = "backing.rs"]
mod backing;
#[path = "capacity.rs"]
mod capacity;
#[path = "cleanup_witness.rs"]
mod cleanup_witness;
#[path = "worker.rs"]
mod worker;

fn admission_witness_is_fresh(
    observation: &crate::launch::StartObservationWitness,
    engine_id: &str,
    docker_root_digest: &str,
    admission_sample_generation_id: &str,
) -> bool {
    observation.is_fresh_for_admission(
        engine_id,
        docker_root_digest,
        admission_sample_generation_id,
    )
}

/// Measure both started containers and build the binding from one trusted plan.
pub(crate) async fn observe_storage_resources<P: ImageProvider>(
    docker: &Docker,
    journal: &ConnectionJournal,
    host_journal: &Journal,
    provider: &P,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    prepared: &PreparedAcquiredBootstrap,
    started: &StartedBootstrapPair,
    initialization: &VerifiedStorageInitializationReady,
) -> Result<VerifiedStorageResourceBinding, HostError> {
    let deadline = Instant::now() + STORAGE_OBSERVATION_TIMEOUT;
    let acquired = prepared.acquired();
    let receipt = started.receipt();
    if !started.runner_stdin_open()
        || !started.jit_withheld()
        || receipt.reservation_id() != acquired.reservation_id()
        || receipt.operation_id() != acquired.intent_id()
        || receipt.engine_id() != engine.engine_id()
        || receipt.docker_root_digest() != engine.docker_root_digest()
        || !engine.matches_images(images)
        || !admission_witness_is_fresh(
            acquired.observation(),
            engine.engine_id(),
            engine.docker_root_digest(),
            acquired.sample_generation_id(),
        )
    {
        return Err(HostError::Identity);
    }
    let plan = journal
        .storage_provisioning_plan(acquired.reservation_id())
        .await?
        .ok_or(HostError::Identity)?;
    validate_plan(
        &plan,
        acquired.reservation_id(),
        acquired.intent_id(),
        acquired.sample_generation_id(),
        engine,
    )?;

    let runner = super::observe_worker_storage(
        docker,
        engine,
        images,
        started.runner_container_id(),
        velnor_resource_probe::worker_binding::WorkerRole::Runner,
        deadline,
    )
    .await?;
    let dind = super::observe_worker_storage(
        docker,
        engine,
        images,
        started.dind_container_id(),
        velnor_resource_probe::worker_binding::WorkerRole::Dind,
        deadline,
    )
    .await?;
    let witness = acquired.observation();
    validate_guest_identity(
        runner.observation(),
        dind.observation(),
        witness.storage_mount().guest_boot_id(),
    )?;

    let subjects = OwnedCgroupSubjects::new(
        OwnedContainerSubjects::new(
            started.runner_container_id().to_owned(),
            runner.init_host_pid(),
            runner.observation().init_start_ticks,
            runner.helper_host_pid(),
            runner.observation().process_start_ticks,
        )?,
        OwnedContainerSubjects::new(
            started.dind_container_id().to_owned(),
            dind.init_host_pid(),
            dind.observation().init_start_ticks,
            dind.helper_host_pid(),
            dind.observation().process_start_ticks,
        )?,
    )?;
    let owned = crate::launch::resource_probe::observe_owned_cgroups_with_provider(
        docker,
        host_journal,
        provider,
        &subjects,
    )
    .await?;
    let owned_observation = owned.observation();
    if owned.engine_id() != engine.engine_id()
        || owned.docker_root_digest() != engine.docker_root_digest()
        || owned.cleanup().operation_id() != owned.operation_id()
        || owned.cleanup().engine_id() != engine.engine_id()
        || owned.cleanup().docker_root_digest() != engine.docker_root_digest()
        || owned.cleanup().container_id().is_empty()
    {
        return Err(HostError::Ownership);
    }
    validate_owned_sample(owned_observation, witness, engine)?;

    let mut measured_attachments = worker_attachments(
        runner.observation(),
        started.runner_container_id(),
        StorageObserver::Runner,
        engine,
    )?;
    measured_attachments.extend(worker_attachments(
        dind.observation(),
        started.dind_container_id(),
        StorageObserver::Dind,
        engine,
    )?);
    measured_attachments.push(probe_attachment(owned_observation, engine)?);
    let root_backings = root_backing_mappings(
        &plan,
        journal,
        engine,
        images,
        &measured_attachments,
        owned_observation,
        witness,
    )?;
    apply_backing_capacity(&mut measured_attachments, &root_backings, owned_observation)?;
    let roles = observed_roles(&plan, &measured_attachments)?;
    let cleanup_observer_binding = cleanup_witness::capture_cleanup_binding(
        docker,
        journal,
        engine,
        images,
        prepared,
        started.runner_container_id(),
        started.dind_container_id(),
        &plan,
        owned_observation,
        &measured_attachments,
        initialization,
        deadline,
    )
    .await?;
    let attachments = measured_attachments
        .into_iter()
        .map(|item| item.binding)
        .collect();
    let cgroups = vec![
        observed_cgroup(
            runner.observation(),
            BootstrapContainerRole::Runner,
            started.runner_container_id(),
        )?,
        observed_cgroup(
            dind.observation(),
            BootstrapContainerRole::Dind,
            started.dind_container_id(),
        )?,
    ];
    let binding = VerifiedStorageResourceBinding {
        reservation_id: acquired.reservation_id(),
        operation_id: acquired.intent_id(),
        engine_id: engine.engine_id().to_owned(),
        docker_root_digest: engine.docker_root_digest().to_owned(),
        sample_generation_id: plan.sample_generation_id().to_owned(),
        profile_generation_id: plan.profile_generation_id().to_owned(),
        profile_digest: plan.profile_digest().to_owned(),
        runner_container_id: started.runner_container_id().to_owned(),
        dind_container_id: started.dind_container_id().to_owned(),
        instance_binding_digest: instance_binding_digest(
            &plan,
            started.runner_container_id(),
            started.dind_container_id(),
            witness.storage_mount().guest_boot_id(),
            &runner,
            &dind,
            owned_observation,
        )?,
        roles,
        attachments,
        cgroups,
        root_backings,
        guest_boot_id: witness.storage_mount().guest_boot_id().to_owned(),
        cleanup_observer_binding,
    };
    let runner_finished = super::finish_worker_storage_observation(docker, runner, deadline).await;
    let dind_finished = super::finish_worker_storage_observation(docker, dind, deadline).await;
    runner_finished?;
    dind_finished?;
    super::super::validation::validate_storage_resource_binding(&binding)?;
    Ok(binding)
}

#[cfg(test)]
#[path = "aggregate_tests.rs"]
mod tests;
