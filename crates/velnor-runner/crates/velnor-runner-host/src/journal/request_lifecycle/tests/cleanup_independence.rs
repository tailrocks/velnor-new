use super::helpers::{
    Scratch, binding, created_scope, event_source, finish_initialized_listener_delivery,
    provision_acquired_request_with_initialization,
    provision_acquired_request_with_initialization_for_repository, record_owned_network_cleanup,
    record_owned_storage_cleanup, runner_name,
};
use crate::error::HostError;
use crate::journal::{CapacityLimits, ConnectionJournal, Journal, ReservationState};
use velnor_runner_github::RunnerReference;

const JOB_MEMORY: u64 = 8_589_934_592;
const TWO_JOB_MEMORY: u64 = 51_539_607_552;

#[tokio::test]
async fn cleanup_releases_one_charge_without_changing_another_live_pair() -> Result<(), HostError> {
    let (_scratch, journal, scope, binding) = setup_two_job_capacity().await?;
    let first = provision_acquired_request_with_initialization(&scope, 70, '8', '8').await?;
    let second = provision_acquired_request_with_initialization(&scope, 71, '9', '9').await?;
    let first_runner = runner('8', 701);
    let second_runner = runner('9', 702);

    finish_initialized_listener_delivery(&scope, &binding, &first, &first_runner, 70).await?;
    finish_initialized_listener_delivery(&scope, &binding, &second, &second_runner, 71).await?;
    let second_network_id =
        assert_distinct_reserved_pairs(&journal, &scope, &first, &second).await?;
    start_both_jobs(
        JobStartInput {
            scope: &scope,
            fixture: &first,
            runner: &first_runner,
            repository_id: 101,
            message_id: 1_070,
        },
        JobStartInput {
            scope: &scope,
            fixture: &second,
            runner: &second_runner,
            repository_id: 101,
            message_id: 1_071,
        },
    )
    .await?;
    complete_and_clean_first(&scope, &first, &first_runner).await?;
    assert_first_reservation_released(&scope, &first).await?;
    assert_second_pair_remains_live(&journal, &scope, &second, &second_network_id).await?;
    admit_third_job(&journal, &scope, &first, &second).await?;
    Ok(())
}

#[tokio::test]
async fn cleanup_in_one_repository_scope_keeps_other_scope_charged() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(two_job_capacity()).await?;
    let first_binding = binding("essential", 101)?;
    let second_binding = binding("chain", 202)?;
    let first_scope = created_scope(&journal, &first_binding, 501).await?;
    let second_scope = created_scope(&journal, &second_binding, 502).await?;
    let first = provision_acquired_request_with_initialization(&first_scope, 70, 'b', 'b').await?;
    let second = provision_acquired_request_with_initialization_for_repository(
        &second_scope,
        71,
        'c',
        'c',
        202,
    )
    .await?;
    let first_runner = runner('b', 801);
    let second_runner = runner_for_set('c', 802, 502);

    finish_initialized_listener_delivery(&first_scope, &first_binding, &first, &first_runner, 70)
        .await?;
    finish_initialized_listener_delivery(
        &second_scope,
        &second_binding,
        &second,
        &second_runner,
        71,
    )
    .await?;
    start_both_jobs(
        JobStartInput {
            scope: &first_scope,
            fixture: &first,
            runner: &first_runner,
            repository_id: 101,
            message_id: 1_070,
        },
        JobStartInput {
            scope: &second_scope,
            fixture: &second,
            runner: &second_runner,
            repository_id: 202,
            message_id: 1_071,
        },
    )
    .await?;
    complete_and_clean_first(&first_scope, &first, &first_runner).await?;

    assert_first_reservation_released(&first_scope, &first).await?;
    let surviving = second_scope
        .reservation(&second.reservation_assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(surviving.reservation_id, second.reservation_id);
    assert_eq!(surviving.state, ReservationState::Reserved);
    assert_eq!(surviving.cpu_millicores, 2_000);
    assert_eq!(surviving.memory_bytes, JOB_MEMORY);
    let intent = second_scope.intent(second.intent_id).await?;
    assert_eq!(
        intent.docker_id.as_deref(),
        Some(second.runner_container_id.as_str())
    );
    assert_eq!(
        intent.dind_id.as_deref(),
        Some(second.dind_container_id.as_str())
    );
    assert!(!intent.cleanup_proven);
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}

async fn setup_two_job_capacity() -> Result<
    (
        Scratch,
        Journal,
        ConnectionJournal,
        crate::config::RepositoryConnection,
    ),
    HostError,
> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(two_job_capacity()).await?;
    let binding = binding("cleanup-pair", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    Ok((scratch, journal, scope, binding))
}

fn two_job_capacity() -> CapacityLimits {
    CapacityLimits {
        jobs: 2,
        cpu_millicores: 4_000,
        memory_bytes: TWO_JOB_MEMORY,
    }
}

async fn assert_distinct_reserved_pairs(
    journal: &Journal,
    scope: &ConnectionJournal,
    first: &super::helpers::JobFactsReaderFixture,
    second: &super::helpers::JobFactsReaderFixture,
) -> Result<String, HostError> {
    assert_ne!(first.reservation_id, second.reservation_id);
    assert_ne!(first.intent_id, second.intent_id);
    assert_ne!(first.runner_container_id, second.runner_container_id);
    assert_ne!(first.dind_container_id, second.dind_container_id);
    let first_network = scope
        .storage_network_recovery_identity(first.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let second_network = scope
        .storage_network_recovery_identity(second.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let second_network_id = second_network
        .network_id()
        .ok_or(HostError::Journal)?
        .to_owned();
    assert_ne!(first_network.network_id(), Some(second_network_id.as_str()));
    assert_eq!(journal.occupied_reservation_count().await?, 2);
    for fixture in [first, second] {
        let row = scope
            .reservation(&fixture.reservation_assignment)
            .await?
            .ok_or(HostError::Journal)?;
        assert_eq!(row.reservation_id, fixture.reservation_id);
        assert_eq!(row.state, ReservationState::Reserved);
        assert_eq!(row.cpu_millicores, 2_000);
        assert_eq!(row.memory_bytes, JOB_MEMORY);
    }
    Ok(second_network_id)
}

struct JobStartInput<'a> {
    scope: &'a ConnectionJournal,
    fixture: &'a super::helpers::JobFactsReaderFixture,
    runner: &'a RunnerReference,
    repository_id: i64,
    message_id: i64,
}

async fn start_both_jobs(
    first: JobStartInput<'_>,
    second: JobStartInput<'_>,
) -> Result<(), HostError> {
    start_job(first).await?;
    start_job(second).await
}

async fn start_job(job: JobStartInput<'_>) -> Result<(), HostError> {
    assert_eq!(
        job.scope
            .record_job_started(
                &job.fixture.assignment,
                job.message_id,
                &event_source(job.repository_id, job.message_id),
                job.runner,
                None,
            )
            .await?,
        job.fixture.intent_id
    );
    Ok(())
}

async fn complete_and_clean_first(
    scope: &ConnectionJournal,
    first: &super::helpers::JobFactsReaderFixture,
    first_runner: &RunnerReference,
) -> Result<(), HostError> {
    assert_eq!(
        scope
            .record_job_completed(
                &first.assignment,
                1_170,
                &event_source(101, 1_070),
                Some("success"),
                Some(first_runner),
                None,
            )
            .await?,
        Some(first.intent_id)
    );
    let claim = scope
        .claim_completion_cleanup(first.intent_id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    if !scope
        .record_completion_runner_absent(first.intent_id, claim.generation)
        .await?
    {
        return Err(HostError::Journal);
    }
    record_owned_storage_cleanup(scope, &first.provisioned).await?;
    record_owned_network_cleanup(scope, first.reservation_id).await?;
    scope.record_remote_cleanup(first.reservation_id).await?;
    scope.record_local_cleanup(first.reservation_id).await?;
    settle_metadata(scope, first).await?;
    Ok(())
}

async fn settle_metadata(
    scope: &ConnectionJournal,
    first: &super::helpers::JobFactsReaderFixture,
) -> Result<(), HostError> {
    let settlement =
        super::helpers::create_cleaned_settlement_probe(scope, &first.provisioned, 'd').await?;
    let sample = super::helpers::settlement_sample(scope, &settlement).await?;
    assert_eq!(
        scope
            .record_storage_metadata_settlement(&settlement, sample)
            .await?,
        crate::journal::MetadataSettlementResult::Settled
    );
    Ok(())
}

async fn assert_second_pair_remains_live(
    journal: &Journal,
    scope: &ConnectionJournal,
    second: &super::helpers::JobFactsReaderFixture,
    expected_network_id: &str,
) -> Result<(), HostError> {
    let row = scope
        .reservation(&second.reservation_assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(row.reservation_id, second.reservation_id);
    assert_eq!(row.state, ReservationState::Reserved);
    assert_eq!(row.cpu_millicores, 2_000);
    assert_eq!(row.memory_bytes, JOB_MEMORY);
    let network = scope
        .storage_network_recovery_identity(second.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(network.network_id(), Some(expected_network_id));
    assert_eq!(network.remove_state(), "not_requested");
    let intent = scope.intent(second.intent_id).await?;
    assert_eq!(
        intent.docker_id.as_deref(),
        Some(second.runner_container_id.as_str())
    );
    assert_eq!(
        intent.dind_id.as_deref(),
        Some(second.dind_container_id.as_str())
    );
    assert!(!intent.cleanup_proven);
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}

async fn assert_first_reservation_released(
    scope: &ConnectionJournal,
    first: &super::helpers::JobFactsReaderFixture,
) -> Result<(), HostError> {
    let released = scope
        .reservation(&first.reservation_assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(released.reservation_id, first.reservation_id);
    assert_eq!(released.state, ReservationState::Released);
    Ok(())
}

async fn admit_third_job(
    journal: &Journal,
    scope: &ConnectionJournal,
    first: &super::helpers::JobFactsReaderFixture,
    second: &super::helpers::JobFactsReaderFixture,
) -> Result<(), HostError> {
    journal.configure_capacity(two_job_capacity()).await?;
    let third = provision_acquired_request_with_initialization(scope, 72, 'a', 'a').await?;
    assert_ne!(third.reservation_id, first.reservation_id);
    assert_ne!(third.reservation_id, second.reservation_id);
    assert_eq!(journal.occupied_reservation_count().await?, 2);
    let surviving = scope
        .reservation(&second.reservation_assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(surviving.reservation_id, second.reservation_id);
    assert_eq!(surviving.state, ReservationState::Reserved);
    assert_eq!(surviving.cpu_millicores, 2_000);
    assert_eq!(surviving.memory_bytes, JOB_MEMORY);
    Ok(())
}

fn runner(nonce: char, id: i64) -> RunnerReference {
    runner_for_set(nonce, id, 501)
}

fn runner_for_set(nonce: char, id: i64, set_id: i64) -> RunnerReference {
    RunnerReference {
        id,
        name: runner_name(nonce),
        runner_scale_set_id: set_id,
    }
}
