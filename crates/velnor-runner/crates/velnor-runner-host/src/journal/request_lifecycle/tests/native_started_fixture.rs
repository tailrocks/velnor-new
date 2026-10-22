use super::{NativeStartedIntakeFixture, helpers};

/// Build the durable state required for a native Started message.
pub(crate) async fn native_started_intake_fixture()
-> Result<NativeStartedIntakeFixture, crate::error::HostError> {
    Box::pin(native_started_intake_fixture_with_filesystem_digest(None)).await
}

pub(crate) async fn native_started_intake_fixture_with_filesystem_digest(
    filesystem_instance_digest: Option<String>,
) -> Result<NativeStartedIntakeFixture, crate::error::HostError> {
    const REQUEST_ID: i64 = 101;
    const MESSAGE_ID: i64 = 1_501;
    const SET_ID: i64 = 501;
    const REPOSITORY_ID: i64 = 101;
    const RUN_ID: i64 = REQUEST_ID + 1_000;

    let scratch = helpers::Scratch::new()?;
    let (journal, scope, binding) =
        native_started_fixture_scope(&scratch, SET_ID, REPOSITORY_ID).await?;
    let (initialized, runner, channel) = prepare_native_started_fixture(
        &scope,
        &binding,
        REQUEST_ID,
        SET_ID,
        filesystem_instance_digest.as_deref(),
    )
    .await?;
    let source = helpers::event_source(REPOSITORY_ID, RUN_ID);
    let database_path = scratch.file();

    Ok(NativeStartedIntakeFixture {
        _scratch: scratch,
        journal,
        database_path,
        scope,
        request_id: REQUEST_ID,
        message_id: MESSAGE_ID,
        intent_id: initialized.intent_id,
        reservation_id: initialized.reservation_id,
        provisioned: initialized.provisioned.clone(),
        assignment: initialized.assignment,
        reservation_assignment: initialized.provisioned.assignment.clone(),
        runner,
        source,
        channel,
    })
}

async fn native_started_fixture_scope(
    scratch: &helpers::Scratch,
    set_id: i64,
    repository_id: i64,
) -> Result<
    (
        crate::journal::Journal,
        crate::journal::ConnectionJournal,
        crate::config::RepositoryConnection,
    ),
    crate::error::HostError,
> {
    use crate::journal::{CapacityLimits, Journal, OperationalCapacityLimits};

    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 25_769_803_776,
            },
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    let binding = helpers::binding("native-started-intake", repository_id)?;
    let scope = helpers::created_scope(&journal, &binding, set_id).await?;
    Ok((journal, scope, binding))
}

async fn prepare_native_started_fixture(
    scope: &crate::journal::ConnectionJournal,
    binding: &crate::config::RepositoryConnection,
    request_id: i64,
    set_id: i64,
    filesystem_instance_digest: Option<&str>,
) -> Result<
    (
        helpers::JobFactsReaderFixture,
        velnor_runner_github::RunnerReference,
        crate::worker::execution_authorization::VerifiedProofChannelBinding,
    ),
    crate::error::HostError,
> {
    let initialized =
        initialize_native_started_request(scope, request_id, filesystem_instance_digest).await?;
    validate_initialized_reader_fixture(scope, &initialized).await?;
    let runner = velnor_runner_github::RunnerReference {
        id: 701,
        name: helpers::runner_name('a'),
        runner_scale_set_id: set_id,
    };
    prepare_native_started_job(
        scope,
        binding,
        &initialized,
        &runner,
        request_id,
        filesystem_instance_digest,
    )
    .await?;
    let channel = native_started_proof_channel(scope, &initialized, &runner, request_id, set_id)?;
    helpers::publish_test_execution_challenge(scope, &channel).await?;
    helpers::finish_listener_delivery_after_challenge(scope, &initialized.provisioned, &runner)
        .await?;
    Ok((initialized, runner, channel))
}

async fn validate_initialized_reader_fixture(
    scope: &crate::journal::ConnectionJournal,
    initialized: &helpers::JobFactsReaderFixture,
) -> Result<(), crate::error::HostError> {
    let reservation = scope
        .reservation(&initialized.reservation_assignment)
        .await?
        .ok_or(crate::error::HostError::Journal)?;
    let claim = &initialized.claim;
    let handoff = &initialized.handoff;
    let observed = &initialized.observed;
    let ready = &initialized.ready;
    if reservation.reservation_id != initialized.reservation_id
        || reservation.initial_sample_generation_id.as_deref()
            != Some(initialized.sample_generation_id.as_str())
        || claim.connection_id() != scope.connection_id().as_str()
        || claim.reservation_id() != reservation.reservation_id
        || claim.operation_id() != initialized.intent_id
        || handoff.connection_id() != scope.connection_id().as_str()
        || handoff.reservation_id() != claim.reservation_id()
        || handoff.operation_id() != claim.operation_id()
        || handoff.generation() != claim.generation()
        || handoff.engine_id() != claim.engine_id()
        || handoff.docker_root_digest() != claim.docker_root_digest()
        || claim.metadata_volume_name() != Some(handoff.metadata_volume_name())
        || claim.metadata_volume_id() != Some(handoff.metadata_volume_id())
        || ready.connection_id() != scope.connection_id().as_str()
        || ready.reservation_id() != reservation.reservation_id
        || ready.operation_id() != initialized.intent_id
        || ready.layout_version() != claim.layout_version()
        || ready.layout_digest() != handoff.layout_digest()
        || observed.layout_digest() != handoff.layout_digest()
        || observed.ready_digest() != handoff.ready_digest()
        || ready.job_facts() != observed.job_facts()
        || ready.metadata_root() != observed.metadata()
        || ready.runtime_identity() != observed.runtime_identity()
    {
        return Err(crate::error::HostError::Identity);
    }
    Ok(())
}

async fn initialize_native_started_request(
    scope: &crate::journal::ConnectionJournal,
    request_id: i64,
    filesystem_instance_digest: Option<&str>,
) -> Result<helpers::JobFactsReaderFixture, crate::error::HostError> {
    match filesystem_instance_digest {
        Some(digest) => {
            helpers::provision_acquired_request_with_initialization_and_filesystem_digest(
                scope,
                request_id,
                'a',
                'a',
                digest.to_owned(),
            )
            .await
        }
        None => {
            helpers::provision_acquired_request_with_initialization(scope, request_id, 'a', 'a')
                .await
        }
    }
}

async fn prepare_native_started_job(
    scope: &crate::journal::ConnectionJournal,
    binding: &crate::config::RepositoryConnection,
    initialized: &helpers::JobFactsReaderFixture,
    runner: &velnor_runner_github::RunnerReference,
    request_id: i64,
    filesystem_instance_digest: Option<&str>,
) -> Result<(), crate::error::HostError> {
    if let Some(digest) = filesystem_instance_digest {
        helpers::prepare_jit_and_job_facts_with_filesystem_digest(
            scope,
            &initialized.provisioned,
            runner,
            request_id,
            digest,
        )
        .await?;
    } else {
        helpers::prepare_jit_and_job_facts(scope, &initialized.provisioned, runner, request_id)
            .await?;
    }
    let config = crate::config::HostConfig::test_with_job_authority_policy(binding)?;
    scope.install_job_authority_policy(&config).await
}

fn native_started_proof_channel(
    scope: &crate::journal::ConnectionJournal,
    initialized: &helpers::JobFactsReaderFixture,
    runner: &velnor_runner_github::RunnerReference,
    request_id: i64,
    set_id: i64,
) -> Result<
    crate::worker::execution_authorization::VerifiedProofChannelBinding,
    crate::error::HostError,
> {
    crate::worker::execution_authorization::test_channel_for_ready_job_facts(
        crate::worker::execution_authorization::TestProofChannelIdentity {
            reservation_id: initialized.reservation_id,
            operation_id: initialized.intent_id,
            connection_id: scope.connection_id().as_str().to_owned(),
            scale_set_id: set_id,
            witness_request_id: request_id,
            engine_id: helpers::TEST_ENGINE_ID.to_owned(),
            docker_root_digest: initialized.docker_root_digest.clone(),
            runner_id: runner.id,
            runner_name: runner.name.clone(),
            runner_container_id: initialized.runner_container_id.clone(),
            dind_container_id: initialized.dind_container_id.clone(),
            instance_binding_digest: initialized.instance_binding_digest.clone(),
        },
        &initialized.ready,
    )
}
