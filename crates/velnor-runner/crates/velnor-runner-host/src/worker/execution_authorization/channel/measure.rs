//! Measure the proof channel inside the owned Runner container.

use std::time::Instant;

use bollard::Docker;
use velnor_resource_probe::execution_channel_wire::{
    ExecutionChannelContext, ExecutionChannelObservation,
};
use velnor_runner_github::RunnerReference;

use crate::{
    error::HostError,
    journal::StorageRole,
    launch::{
        SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::{
            BootstrapStartReceipt, VerifiedCreatedStorageObject, VerifiedStorageResourceBinding,
        },
        steps::AcquiredProvisioning,
    },
};

use super::VerifiedProofChannelBinding;
mod acknowledgment;
mod protocol;
pub(super) use protocol::{decode_line, drain_empty_output, encode_line, read_stdout_line, within};
mod exchange;
use exchange::{ChannelExchange, exchange_channel_record};

mod attachments;
use attachments::make_mount;

const CONTROL_PATH: &str = "/opt/velnor/source-proof/control";
const CONTROL_SOURCE: &str = "source-proof/control";
const INBOX_PATH: &str = "/opt/velnor/source-proof/inbox";
const INBOX_SOURCE: &str = "source-proof/inbox";
const JOB_FACTS_PATH: &str = "/opt/velnor/job-facts";
const JOB_FACTS_SOURCE: &str = "job-facts";

pub(crate) struct ProofChannelMeasurement<'a> {
    pub(crate) engine: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
    pub(crate) acquired: &'a AcquiredProvisioning,
    pub(crate) started: &'a BootstrapStartReceipt,
    pub(crate) storage: &'a VerifiedStorageResourceBinding,
    pub(crate) metadata: &'a VerifiedCreatedStorageObject,
    pub(crate) runner: &'a RunnerReference,
    pub(crate) deadline: Instant,
}

/// Measure the exact Runner channel after the host starts the owned container pair.
pub(crate) async fn measure_proof_channel(
    docker: &Docker,
    measurement: ProofChannelMeasurement<'_>,
) -> Result<VerifiedProofChannelBinding, HostError> {
    let ProofChannelMeasurement {
        engine,
        images,
        acquired,
        started,
        storage,
        metadata,
        runner,
        deadline,
    } = measurement;
    validate_inputs(engine, acquired, started, storage, metadata, runner)?;
    within(deadline, images.verify_for_engine(docker, engine)).await??;
    inspect_runner(docker, images, started.runner_container_id(), deadline).await?;

    let operation_id = uuid::Uuid::new_v4().simple().to_string();
    let request = ExecutionChannelContext {
        schema_version: 3,
        operation_id: operation_id.clone(),
        expected_guest_boot_id: storage.guest_boot_id().to_owned(),
    };
    let observation = exchange_channel_record(
        docker,
        ChannelExchange {
            container_id: started.runner_container_id(),
            request: &request,
            storage,
            started,
            deadline,
        },
    )
    .await?;
    inspect_runner(docker, images, started.runner_container_id(), deadline).await?;
    make_binding(acquired, started, storage, metadata, runner, &observation)
}

fn validate_inputs(
    engine: &SelectedEngineContext,
    acquired: &AcquiredProvisioning,
    started: &BootstrapStartReceipt,
    storage: &VerifiedStorageResourceBinding,
    metadata: &VerifiedCreatedStorageObject,
    runner: &RunnerReference,
) -> Result<(), HostError> {
    let assignment = acquired.request();
    let request_id = assignment.request_id().ok_or(HostError::Identity)?;
    if !started.runner_stdin_open()
        || !started.jit_withheld()
        || started.reservation_id() != acquired.reservation_id()
        || started.operation_id() != acquired.intent_id()
        || started.reservation_id() != storage.reservation_id()
        || started.operation_id() != storage.operation_id()
        || started.engine_id() != engine.engine_id()
        || started.docker_root_digest() != engine.docker_root_digest()
        || started.runner_container_id() != storage.runner_container_id()
        || started.dind_container_id() != storage.dind_container_id()
        || runner.id <= 0
        || runner.name != acquired.runner_name()
        || runner.runner_scale_set_id != assignment.scale_set_id()
        || metadata.reservation_id() != acquired.reservation_id()
        || metadata.operation_id() != acquired.intent_id()
        || metadata.engine_id() != engine.engine_id()
        || metadata.docker_root_digest() != engine.docker_root_digest()
        || metadata.role() != StorageRole::EngineMetadata
        || storage.instance_binding_digest().is_empty()
        || storage.guest_boot_id().is_empty()
        || request_id <= 0
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn inspect_runner(
    docker: &Docker,
    images: &VerifiedWorkerImages,
    container_id: &str,
    deadline: Instant,
) -> Result<(), HostError> {
    let inspect = within(deadline, docker.inspect_container(container_id, None))
        .await?
        .map_err(|_| HostError::Docker)?;
    if inspect.id.as_deref() != Some(container_id)
        || inspect.image.as_deref() != Some(images.runner().runtime_id())
        || inspect.state.and_then(|state| state.running) != Some(true)
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn validate_observation(
    observation: &ExecutionChannelObservation,
    operation_id: &str,
    expected_guest_boot_id: &str,
) -> Result<(), HostError> {
    if observation.schema_version != 3
        || observation.operation_id != operation_id
        || observation.guest_boot_id != expected_guest_boot_id
        || observation.mount_namespace_id == 0
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn make_binding(
    acquired: &AcquiredProvisioning,
    started: &BootstrapStartReceipt,
    storage: &VerifiedStorageResourceBinding,
    metadata: &VerifiedCreatedStorageObject,
    runner: &RunnerReference,
    observation: &ExecutionChannelObservation,
) -> Result<VerifiedProofChannelBinding, HostError> {
    let assignment = acquired.request();
    let witness_request_id = assignment.request_id().ok_or(HostError::Identity)?;
    let binding = VerifiedProofChannelBinding {
        reservation_id: acquired.reservation_id(),
        operation_id: acquired.intent_id(),
        connection_id: assignment.connection_id().as_str().to_owned(),
        scale_set_id: assignment.scale_set_id(),
        witness_request_id,
        engine_id: started.engine_id().to_owned(),
        docker_root_digest: started.docker_root_digest().to_owned(),
        runner_id: runner.id,
        runner_name: runner.name.clone(),
        runner_container_id: started.runner_container_id().to_owned(),
        dind_container_id: started.dind_container_id().to_owned(),
        instance_binding_digest: storage.instance_binding_digest().to_owned(),
        metadata_volume_name: metadata.docker_name().to_owned(),
        metadata_volume_id: metadata.docker_id().to_owned(),
        control: make_mount(
            &observation.control,
            CONTROL_PATH,
            CONTROL_SOURCE,
            storage,
            started.runner_container_id(),
            true,
            StorageRole::EngineMetadata,
        )?,
        control_directory: observation.control.clone(),
        inbox: make_mount(
            &observation.inbox,
            INBOX_PATH,
            INBOX_SOURCE,
            storage,
            started.runner_container_id(),
            false,
            StorageRole::RunnerHome,
        )?,
        job_facts: make_mount(
            &observation.job_facts,
            JOB_FACTS_PATH,
            JOB_FACTS_SOURCE,
            storage,
            started.runner_container_id(),
            true,
            StorageRole::EngineMetadata,
        )?,
        observation: observation.clone(),
        dind_attachment_absent: true,
        channel_digest: String::new(),
    };
    let mut binding = binding;
    binding.channel_digest = binding.computed_digest()?;
    if binding.valid() {
        Ok(binding)
    } else {
        Err(HostError::Identity)
    }
}
