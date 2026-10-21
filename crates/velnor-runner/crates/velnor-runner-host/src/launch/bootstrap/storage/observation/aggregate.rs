use std::time::{Duration, Instant};

use backing::{apply_backing_capacity, root_backing_mappings, validate_owned_sample};
use capacity::{instance_binding_digest, observed_cgroup, observed_roles};
use worker::{probe_attachment, validate_guest_identity, validate_plan, worker_attachments};

use crate::{
    error::HostError,
    journal::{StorageObserver, StorageProvisioningPlan},
    launch::{
        StartObservationWitness,
        bootstrap::{
            BootstrapContainerRole, PreparedAcquiredBootstrap, StartedBootstrapPair,
            VerifiedStorageResourceBinding,
        },
        resource_probe::{
            ImageProvider, OwnedCgroupSubjects, OwnedContainerSubjects, VerifiedOwnedCgroups,
        },
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
    observation: &StartObservationWitness,
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
    context: &super::StorageObservationContext<'_, P>,
    prepared: &PreparedAcquiredBootstrap,
    started: &StartedBootstrapPair,
    initialization: &VerifiedStorageInitializationReady,
) -> Result<VerifiedStorageResourceBinding, HostError> {
    let deadline = Instant::now() + STORAGE_OBSERVATION_TIMEOUT;
    validate_started_identity(context, prepared, started)?;
    let plan = load_plan(context, prepared).await?;
    let run = ObservationRun {
        context,
        prepared,
        started,
        initialization,
        plan: &plan,
        deadline,
    };
    let workers = observe_workers(&run).await?;
    let owned = observe_owned_cgroups(&run, &workers).await?;
    let facts = collect_storage_facts(&run, &workers, &owned).await?;
    let binding = assemble_binding(&run, &workers, &owned, facts)?;
    let runner_finished =
        super::finish_worker_storage_observation(context, workers.runner, deadline).await;
    let dind_finished =
        super::finish_worker_storage_observation(context, workers.dind, deadline).await;
    runner_finished?;
    dind_finished?;
    super::super::validation::validate_storage_resource_binding(&binding)?;
    Ok(binding)
}

struct ObservationRun<'a, P: ImageProvider> {
    context: &'a super::StorageObservationContext<'a, P>,
    prepared: &'a PreparedAcquiredBootstrap,
    started: &'a StartedBootstrapPair,
    initialization: &'a VerifiedStorageInitializationReady,
    plan: &'a StorageProvisioningPlan,
    deadline: Instant,
}

struct WorkerObservations {
    runner: super::VerifiedWorkerStorageObservation,
    dind: super::VerifiedWorkerStorageObservation,
}

struct StorageFacts {
    roles: Vec<crate::launch::bootstrap::ObservedStorageRole>,
    attachments: Vec<crate::launch::bootstrap::ObservedStorageAttachment>,
    root_backings: Vec<crate::launch::bootstrap::storage::VerifiedRootBackingMapping>,
    cleanup_observer_binding: velnor_resource_probe::cleanup_observer_wire::CleanupObserverBinding,
}

fn validate_started_identity<P: ImageProvider>(
    context: &super::StorageObservationContext<'_, P>,
    prepared: &PreparedAcquiredBootstrap,
    started: &StartedBootstrapPair,
) -> Result<(), HostError> {
    let acquired = prepared.acquired();
    let receipt = started.receipt();
    if !started.runner_stdin_open()
        || !started.jit_withheld()
        || receipt.reservation_id() != acquired.reservation_id()
        || receipt.operation_id() != acquired.intent_id()
        || receipt.engine_id() != context.engine.engine_id()
        || receipt.docker_root_digest() != context.engine.docker_root_digest()
        || !context.engine.matches_images(context.images)
        || !admission_witness_is_fresh(
            acquired.observation(),
            context.engine.engine_id(),
            context.engine.docker_root_digest(),
            acquired.sample_generation_id(),
        )
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn load_plan<P: ImageProvider>(
    context: &super::StorageObservationContext<'_, P>,
    prepared: &PreparedAcquiredBootstrap,
) -> Result<StorageProvisioningPlan, HostError> {
    let acquired = prepared.acquired();
    let plan = context
        .journal
        .storage_provisioning_plan(acquired.reservation_id())
        .await?
        .ok_or(HostError::Identity)?;
    validate_plan(
        &plan,
        acquired.reservation_id(),
        acquired.intent_id(),
        acquired.sample_generation_id(),
        context.engine,
    )?;
    Ok(plan)
}

async fn observe_workers<P: ImageProvider>(
    run: &ObservationRun<'_, P>,
) -> Result<WorkerObservations, HostError> {
    let runner = super::observe_worker_storage(
        run.context,
        run.started.runner_container_id(),
        velnor_resource_probe::worker_binding::WorkerRole::Runner,
        run.deadline,
    )
    .await?;
    let dind = super::observe_worker_storage(
        run.context,
        run.started.dind_container_id(),
        velnor_resource_probe::worker_binding::WorkerRole::Dind,
        run.deadline,
    )
    .await?;
    validate_guest_identity(
        runner.observation(),
        dind.observation(),
        run.prepared
            .acquired()
            .observation()
            .storage_mount()
            .guest_boot_id(),
    )?;
    Ok(WorkerObservations { runner, dind })
}

async fn observe_owned_cgroups<P: ImageProvider>(
    run: &ObservationRun<'_, P>,
    workers: &WorkerObservations,
) -> Result<VerifiedOwnedCgroups, HostError> {
    let subjects = OwnedCgroupSubjects::new(
        container_subject(run.started.runner_container_id(), &workers.runner)?,
        container_subject(run.started.dind_container_id(), &workers.dind)?,
    )?;
    let owned = Box::pin(
        crate::launch::resource_probe::observe_owned_cgroups_with_provider(
            run.context.docker,
            run.context.host_journal,
            run.context.provider,
            &subjects,
        ),
    )
    .await?;
    let observation = owned.observation();
    if owned.engine_id() != run.context.engine.engine_id()
        || owned.docker_root_digest() != run.context.engine.docker_root_digest()
        || owned.cleanup().operation_id() != owned.operation_id()
        || owned.cleanup().engine_id() != run.context.engine.engine_id()
        || owned.cleanup().docker_root_digest() != run.context.engine.docker_root_digest()
        || owned.cleanup().container_id().is_empty()
    {
        return Err(HostError::Ownership);
    }
    validate_owned_sample(
        observation,
        run.prepared.acquired().observation(),
        run.context.engine,
    )?;
    Ok(owned)
}

fn container_subject(
    container_id: &str,
    worker: &super::VerifiedWorkerStorageObservation,
) -> Result<crate::launch::resource_probe::OwnedContainerSubjects, HostError> {
    OwnedContainerSubjects::new(
        container_id.to_owned(),
        worker.init_host_pid(),
        worker.observation().init_start_ticks,
        worker.helper_host_pid(),
        worker.observation().process_start_ticks,
    )
}

async fn collect_storage_facts<P: ImageProvider>(
    run: &ObservationRun<'_, P>,
    workers: &WorkerObservations,
    owned: &VerifiedOwnedCgroups,
) -> Result<StorageFacts, HostError> {
    let observation = owned.observation();
    let mut measured = worker_attachments(
        workers.runner.observation(),
        run.started.runner_container_id(),
        StorageObserver::Runner,
        run.context.engine,
    )?;
    measured.extend(worker_attachments(
        workers.dind.observation(),
        run.started.dind_container_id(),
        StorageObserver::Dind,
        run.context.engine,
    )?);
    measured.push(probe_attachment(observation, run.context.engine)?);
    let backing_context = backing::RootBackingContext {
        plan: run.plan,
        journal: run.context.journal,
        engine: run.context.engine,
        images: run.context.images,
        witness: run.prepared.acquired().observation(),
    };
    let root_backings = root_backing_mappings(&backing_context, &measured, observation)?;
    apply_backing_capacity(&mut measured, &root_backings, observation)?;
    let roles = observed_roles(run.plan, &measured)?;
    let cleanup_context = cleanup_witness::CleanupObservationContext {
        docker: run.context.docker,
        journal: run.context.journal,
        engine: run.context.engine,
        images: run.context.images,
        prepared: run.prepared,
        initialization: run.initialization,
    };
    let cleanup_observer_binding = cleanup_witness::capture_cleanup_binding(
        &cleanup_context,
        run.started.runner_container_id(),
        run.started.dind_container_id(),
        run.plan,
        observation,
        &measured,
        run.deadline,
    )
    .await?;
    Ok(StorageFacts {
        roles,
        attachments: measured.into_iter().map(|item| item.binding).collect(),
        root_backings,
        cleanup_observer_binding,
    })
}

fn assemble_binding<P: ImageProvider>(
    run: &ObservationRun<'_, P>,
    workers: &WorkerObservations,
    owned: &VerifiedOwnedCgroups,
    facts: StorageFacts,
) -> Result<VerifiedStorageResourceBinding, HostError> {
    let acquired = run.prepared.acquired();
    let witness = acquired.observation();
    let cgroups = vec![
        observed_cgroup(
            workers.runner.observation(),
            BootstrapContainerRole::Runner,
            run.started.runner_container_id(),
        )?,
        observed_cgroup(
            workers.dind.observation(),
            BootstrapContainerRole::Dind,
            run.started.dind_container_id(),
        )?,
    ];
    Ok(VerifiedStorageResourceBinding {
        reservation_id: acquired.reservation_id(),
        operation_id: acquired.intent_id(),
        engine_id: run.context.engine.engine_id().to_owned(),
        docker_root_digest: run.context.engine.docker_root_digest().to_owned(),
        sample_generation_id: run.plan.sample_generation_id().to_owned(),
        profile_generation_id: run.plan.profile_generation_id().to_owned(),
        profile_digest: run.plan.profile_digest().to_owned(),
        runner_container_id: run.started.runner_container_id().to_owned(),
        dind_container_id: run.started.dind_container_id().to_owned(),
        instance_binding_digest: instance_binding_digest(
            run.plan,
            run.started.runner_container_id(),
            run.started.dind_container_id(),
            witness.storage_mount().guest_boot_id(),
            &workers.runner,
            &workers.dind,
            owned.observation(),
        )?,
        roles: facts.roles,
        attachments: facts.attachments,
        cgroups,
        root_backings: facts.root_backings,
        guest_boot_id: witness.storage_mount().guest_boot_id().to_owned(),
        cleanup_observer_binding: facts.cleanup_observer_binding,
    })
}

#[cfg(test)]
#[path = "aggregate_tests.rs"]
mod tests;
