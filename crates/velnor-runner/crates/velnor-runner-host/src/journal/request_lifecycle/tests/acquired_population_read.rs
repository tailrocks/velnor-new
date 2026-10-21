use crate::error::HostError;
use crate::host_controller::{AssignedPopulationAction, plan_assigned_population_with_successors};
use crate::journal::{
    AssignedAcquireResult, AssignmentKey, CapacityLimits, Journal, SourceAuthorizationResult,
};
use velnor_runner_github::RunnerReference;

use super::acquired_recovery::acquired_candidate_fixture;

#[tokio::test]
async fn acquired_population_uses_exact_journal_candidate_and_bounds_start_to_shortfall()
-> Result<(), HostError> {
    let fixture = acquired_candidate_fixture().await?;
    let reservations = fixture.scope.physical_runner_reservations().await?;
    let acquired = fixture.scope.acquired_authorized_requests(64).await?;
    let candidates = [fixture.candidate];

    let one_job = plan_assigned_population_with_successors(
        1,
        fixture.scope.connection_id(),
        fixture.set_id,
        &reservations,
        &acquired,
        &candidates,
        &[],
    );
    assert_eq!(one_job.uncovered, 0, "{one_job:#?} {reservations:#?}");
    assert_eq!(one_job.actions.len(), 1);
    assert_eq!(one_job.blocked, None);

    let plan = plan_assigned_population_with_successors(
        10,
        fixture.scope.connection_id(),
        fixture.set_id,
        &reservations,
        &acquired,
        &candidates,
        &[],
    );
    assert_eq!(plan.owned_coverage, 0);
    assert_eq!(plan.uncovered, 9);
    assert_eq!(plan.blocked, None);
    assert_eq!(
        plan.actions,
        vec![AssignedPopulationAction::ContinueExisting {
            request: fixture.request.clone(),
            reservation_id: candidates[0].reservation_id(),
            candidate_index: 0,
        }]
    );

    let no_demand = plan_assigned_population_with_successors(
        0,
        fixture.scope.connection_id(),
        fixture.set_id,
        &reservations,
        &acquired,
        &candidates,
        &[],
    );
    assert!(no_demand.actions.is_empty());
    assert_eq!(no_demand.uncovered, 0);
    assert_eq!(no_demand.blocked, None);
    assert_eq!(
        reservations[0].reservation.state,
        crate::journal::ReservationState::Reserved
    );
    Ok(())
}

#[tokio::test]
async fn actual_started_request_can_differ_from_the_acquire_witness() -> Result<(), HostError> {
    const WITNESS: i64 = 811;
    const ACTUAL: i64 = 812;
    const SET_ID: i64 = 501;
    let scratch = super::helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = super::helpers::binding("witness-split", 101)?;
    let scope = super::helpers::created_scope(&journal, &binding, SET_ID).await?;
    let initialized =
        super::helpers::provision_acquired_request_with_initialization(&scope, WITNESS, 'a', 'a')
            .await?;
    let runner = RunnerReference {
        id: 701,
        name: super::helpers::runner_name('a'),
        runner_scale_set_id: SET_ID,
    };
    super::helpers::finish_initialized_listener_delivery(
        &scope,
        &binding,
        &initialized,
        &runner,
        WITNESS,
    )
    .await?;

    let actual = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: ACTUAL })
        .await?;
    let source = super::helpers::receipt(&scope, ACTUAL, ACTUAL + 100, 101).await?;
    assert_eq!(
        scope.record_source_authorization(&actual, &source).await?,
        SourceAuthorizationResult::Recorded
    );
    assert!(scope.claim_request_acquire(&actual).await?);
    scope
        .record_request_acquire(&actual, AssignedAcquireResult::Returned(vec![ACTUAL]))
        .await?;
    assert_eq!(
        scope
            .record_job_started(
                &actual,
                905,
                &super::helpers::event_source(101, ACTUAL + 1000),
                &runner,
                Some("opaque-job-812"),
            )
            .await?,
        initialized.intent_id
    );

    let reservations = scope.physical_runner_reservations().await?;
    let row = reservations
        .iter()
        .find(|row| row.reservation_id == initialized.reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(row.witness_request_id, Some(WITNESS));
    assert_eq!(row.started_request_id, Some(ACTUAL));
    assert_eq!(row.runner_request_id, Some(ACTUAL));
    assert_eq!(row.acquired_request, Some(initialized.assignment.clone()));
    assert!(matches!(
        row.ownership,
        crate::journal::PhysicalRunnerOwnership::Known(_)
    ));

    let acquired = scope.acquired_authorized_requests(64).await?;
    let plan = plan_assigned_population_with_successors(
        1,
        scope.connection_id(),
        SET_ID,
        &reservations,
        &acquired,
        &[],
        &[],
    );
    assert_eq!(plan.owned_coverage, 1);
    assert!(plan.actions.is_empty());
    assert_eq!(plan.uncovered, 0);
    assert_eq!(plan.blocked, None);
    Ok(())
}
