use std::path::PathBuf;

use super::{
    JobStartInput, admit_third_job, assert_distinct_reserved_pairs,
    assert_first_reservation_released, assert_second_pair_remains_live, complete_and_clean_first,
    event_source, runner, setup_two_job_capacity, start_both_jobs,
};
use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::journal::{ConnectionJournal, Journal, ReservationState};
use velnor_runner_github::RunnerReference;

struct PairIdentity {
    reservation: i64,
    intent: i64,
    runner_container: String,
    dind_container: String,
}

struct PairState {
    _scratch: super::super::helpers::Scratch,
    database_path: PathBuf,
    journal: Journal,
    scope: ConnectionJournal,
    binding: RepositoryConnection,
    first: super::super::helpers::JobFactsReaderFixture,
    second: super::super::helpers::JobFactsReaderFixture,
    first_runner: RunnerReference,
    first_pair: PairIdentity,
    second_pair: PairIdentity,
    second_network_id: String,
}

#[test]
fn conflicting_completion_cannot_rebind_pair_across_reopen() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "conflicting_completion_cannot_rebind_pair_across_reopen",
        || {
            Box::pin(async {
                Box::pin(conflicting_completion_cannot_rebind_pair_across_reopen_on_fixture_stack())
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn conflicting_completion_cannot_rebind_pair_across_reopen_on_fixture_stack()
-> Result<(), HostError> {
    let state = prepare_pair().await?;
    record_first_completion(&state).await?;
    let intent_id = state.first_pair.intent;
    let state = reopen_pair(state, Some(intent_id)).await?;
    reject_conflicting_replay(&state).await?;
    let state = Box::pin(settle_first_and_reopen(state)).await?;
    assert_first_reservation_released(&state.scope, &state.first).await?;
    assert_second_pair_remains_live(
        &state.journal,
        &state.scope,
        &state.second,
        &state.second_network_id,
    )
    .await?;
    assert_second_identity(&state).await?;
    admit_third_job(&state.journal, &state.scope, &state.first, &state.second).await?;
    Ok(())
}

async fn prepare_pair() -> Result<PairState, HostError> {
    let (scratch, journal, scope, binding) = setup_two_job_capacity().await?;
    let first =
        super::super::helpers::provision_acquired_request_with_initialization(&scope, 70, '8', '8')
            .await?;
    let second =
        super::super::helpers::provision_acquired_request_with_initialization(&scope, 71, '9', '9')
            .await?;
    let first_pair = pair_identity(&first);
    let second_pair = pair_identity(&second);
    let first_runner = runner('8', 701);
    let second_runner = runner('9', 702);
    finish_delivery(&scope, &binding, &first, &first_runner, 70).await?;
    finish_delivery(&scope, &binding, &second, &second_runner, 71).await?;
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
    let database_path = scratch.file();
    Ok(PairState {
        _scratch: scratch,
        database_path,
        journal,
        scope,
        binding,
        first,
        second,
        first_runner,
        first_pair,
        second_pair,
        second_network_id,
    })
}

fn pair_identity(fixture: &super::super::helpers::JobFactsReaderFixture) -> PairIdentity {
    PairIdentity {
        reservation: fixture.reservation_id,
        intent: fixture.intent_id,
        runner_container: fixture.runner_container_id.clone(),
        dind_container: fixture.dind_container_id.clone(),
    }
}

async fn finish_delivery(
    scope: &ConnectionJournal,
    binding: &RepositoryConnection,
    fixture: &super::super::helpers::JobFactsReaderFixture,
    runner: &RunnerReference,
    request_id: i64,
) -> Result<(), HostError> {
    super::super::helpers::finish_initialized_listener_delivery(
        scope, binding, fixture, runner, request_id,
    )
    .await
}

async fn record_first_completion(state: &PairState) -> Result<(), HostError> {
    assert_eq!(
        state
            .scope
            .record_job_completed(
                &state.first.assignment,
                1_170,
                &event_source(101, 1_070),
                Some("success"),
                Some(&state.first_runner),
                None,
            )
            .await?,
        Some(state.first_pair.intent)
    );
    let launch = completed_launch(&state.scope, state.first_pair.intent).await?;
    assert_native_pair(&state.scope, &launch, &state.first_pair).await?;
    assert_eq!(state.journal.occupied_reservation_count().await?, 2);
    Ok(())
}

async fn completed_launch(
    scope: &ConnectionJournal,
    expected_intent_id: i64,
) -> Result<crate::journal::CompletedLaunch, HostError> {
    let mut launches = scope.due_completed_launches(0, 2).await?;
    if launches.len() != 1 || launches[0].intent.id != expected_intent_id {
        return Err(HostError::Journal);
    }
    launches.pop().ok_or(HostError::Journal)
}

async fn assert_native_pair(
    scope: &ConnectionJournal,
    launch: &crate::journal::CompletedLaunch,
    expected: &PairIdentity,
) -> Result<(), HostError> {
    let actual = scope
        .native_completion_reservation(launch)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(actual.reservation_id(), expected.reservation);
    assert_eq!(actual.intent_id(), expected.intent);
    assert_eq!(actual.runner_container_id(), expected.runner_container);
    assert_eq!(actual.dind_container_id(), expected.dind_container);
    Ok(())
}

async fn reopen_pair(
    state: PairState,
    unsettled_intent_id: Option<i64>,
) -> Result<PairState, HostError> {
    let PairState {
        _scratch: scratch,
        database_path,
        journal,
        scope,
        binding,
        first,
        second,
        first_runner,
        first_pair,
        second_pair,
        second_network_id,
    } = state;
    drop(scope);
    drop(journal);
    let journal = Journal::open(&database_path).await?;
    let scope = journal.connection_scope(&binding).await?;
    if let Some(intent_id) = unsettled_intent_id {
        assert_unsettled_pair(&journal, &scope, &first, &second, intent_id, &first_pair).await?;
    } else if !scope.due_completed_launches(0, 1).await?.is_empty() {
        return Err(HostError::Journal);
    }
    Ok(PairState {
        _scratch: scratch,
        database_path,
        journal,
        scope,
        binding,
        first,
        second,
        first_runner,
        first_pair,
        second_pair,
        second_network_id,
    })
}

async fn assert_unsettled_pair(
    journal: &Journal,
    scope: &ConnectionJournal,
    first: &super::super::helpers::JobFactsReaderFixture,
    second: &super::super::helpers::JobFactsReaderFixture,
    intent_id: i64,
    expected: &PairIdentity,
) -> Result<(), HostError> {
    let launch = completed_launch(scope, intent_id).await?;
    assert_native_pair(scope, &launch, expected).await?;
    assert_eq!(
        scope
            .reservation(&first.reservation_assignment)
            .await?
            .ok_or(HostError::Journal)?
            .state,
        ReservationState::Reserved
    );
    assert_eq!(journal.occupied_reservation_count().await?, 2);
    assert_distinct_reserved_pairs(journal, scope, first, second).await?;
    Ok(())
}

async fn reject_conflicting_replay(state: &PairState) -> Result<(), HostError> {
    let conflicting_runner = runner('a', 799);
    assert!(matches!(
        state
            .scope
            .record_job_completed(
                &state.first.assignment,
                1_170,
                &event_source(101, 1_070),
                Some("success"),
                Some(&conflicting_runner),
                None,
            )
            .await,
        Err(HostError::Identity)
    ));
    assert!(
        !state
            .scope
            .intent(state.first_pair.intent)
            .await?
            .cleanup_proven
    );
    assert_unsettled_pair(
        &state.journal,
        &state.scope,
        &state.first,
        &state.second,
        state.first_pair.intent,
        &state.first_pair,
    )
    .await
}

async fn settle_first_and_reopen(state: PairState) -> Result<PairState, HostError> {
    complete_and_clean_first(&state.scope, &state.first, &state.first_runner).await?;
    assert_first_reservation_released(&state.scope, &state.first).await?;
    assert_second_pair_remains_live(
        &state.journal,
        &state.scope,
        &state.second,
        &state.second_network_id,
    )
    .await?;
    reopen_pair(state, None).await
}

async fn assert_second_identity(state: &PairState) -> Result<(), HostError> {
    let reservation = state
        .scope
        .reservation(&state.second.reservation_assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.reservation_id, state.second_pair.reservation);
    assert_eq!(state.second_pair.intent, state.second.intent_id);
    let intent = state.scope.intent(state.second_pair.intent).await?;
    assert_eq!(
        intent.docker_id.as_deref(),
        Some(state.second_pair.runner_container.as_str())
    );
    assert_eq!(
        intent.dind_id.as_deref(),
        Some(state.second_pair.dind_container.as_str())
    );
    Ok(())
}
