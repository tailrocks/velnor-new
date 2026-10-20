use crate::config::ConnectionId;
use crate::journal::{
    AssignmentIdentity, AssignmentKey, IntentState, PhysicalRunnerJitState,
    PhysicalRunnerOwnership, PhysicalRunnerReservation, ReservationSnapshot, ReservationState,
};

use super::{AssignedPopulationBlock, plan_assigned_population};

fn request(request_id: i64) -> AssignmentIdentity {
    AssignmentIdentity::new(
        &ConnectionId::parse("population-test").expect("valid connection"),
        17,
        AssignmentKey::AssignedRequest { request_id },
    )
    .expect("valid request")
}

fn connection() -> ConnectionId {
    ConnectionId::parse("population-test").expect("valid connection")
}

fn reservation(
    reservation_id: i64,
    identity: PhysicalRunnerOwnership,
    jit_state: PhysicalRunnerJitState,
    runner: Option<velnor_runner_github::RunnerReference>,
) -> PhysicalRunnerReservation {
    PhysicalRunnerReservation {
        reservation_id,
        intent_id: reservation_id,
        ownership: identity,
        reservation: ReservationSnapshot {
            reservation_id,
            state: ReservationState::Reserved,
            initial_sample_generation_id: Some("sample-1".to_owned()),
            resume_eligible: false,
            cpu_millicores: 1000,
            memory_bytes: 1024,
        },
        intent_state: Some(IntentState::Done),
        launch_phase: Some(crate::reconcile::LaunchPhase::WorkerReady),
        jit_state,
        requested_runner_name: Some(format!("runner-{reservation_id}")),
        runner,
        witness_request_id: None,
        started_request_id: None,
        completed_request_id: None,
        runner_request_id: None,
        acquired_request: None,
        worker_volume: None,
        runner_container_id: None,
        dind_container_id: None,
        intent_cleanup_proven: false,
        worker_cleanup_proven: false,
        remote_cleanup_proven: false,
        local_cleanup_proven: false,
        completion_cleanup_pending: false,
    }
}

fn known_request(request_id: i64) -> PhysicalRunnerOwnership {
    PhysicalRunnerOwnership::Known(request(request_id))
}

fn remote_runner(subject: &str) -> PhysicalRunnerOwnership {
    PhysicalRunnerOwnership::Known(
        AssignmentIdentity::new(
            &connection(),
            17,
            AssignmentKey::Remote {
                kind: "runner".to_owned(),
                subject: subject.to_owned(),
            },
        )
        .expect("valid runner assignment"),
    )
}

fn runner(id: i64) -> velnor_runner_github::RunnerReference {
    velnor_runner_github::RunnerReference {
        id,
        name: format!("runner-{id}"),
        runner_scale_set_id: 17,
    }
}

#[test]
fn assigned_total_does_not_create_physical_authority_for_an_unbound_request() {
    let rows = [reservation(
        1,
        known_request(101),
        PhysicalRunnerJitState::Started,
        Some(runner(201)),
    )];
    let plan = plan_assigned_population(12, &connection(), 17, &rows, &[request(102)], &[]);
    assert_eq!(plan.assigned_total, 12);
    assert_eq!(plan.owned_coverage, 1);
    assert_eq!(plan.actions, []);
    assert_eq!(plan.uncovered, 11);
    assert_eq!(plan.blocked, Some(AssignedPopulationBlock::ClosedUnproven));
}

#[test]
fn pre_jit_row_without_validated_candidate_does_not_start() {
    let mut row = reservation(
        4,
        known_request(104),
        PhysicalRunnerJitState::Completed,
        None,
    );
    row.jit_state = PhysicalRunnerJitState::Absent;
    row.intent_state = Some(IntentState::Pending);
    row.launch_phase = Some(crate::reconcile::LaunchPhase::ProvisioningHeld);
    row.acquired_request = Some(request(104));
    let plan = plan_assigned_population(1, &connection(), 17, &[row], &[request(104)], &[]);
    assert_eq!(plan.owned_coverage, 0);
    assert_eq!(plan.actions, []);
    assert_eq!(plan.blocked, None);
    assert_eq!(plan.uncovered, 1);
}

#[test]
fn truncated_source_population_preserves_uncovered_demand() {
    let mut row = reservation(14, known_request(114), PhysicalRunnerJitState::Absent, None);
    row.intent_state = Some(IntentState::Pending);
    row.launch_phase = Some(crate::reconcile::LaunchPhase::ProvisioningHeld);
    row.acquired_request = Some(request(114));
    let plan = plan_assigned_population(10, &connection(), 17, &[row], &[request(114)], &[]);
    assert_eq!(plan.owned_coverage, 0);
    assert_eq!(plan.actions, []);
    assert_eq!(plan.uncovered, 10);
    assert_eq!(plan.blocked, None);
}

#[test]
fn assigned_count_gap_without_exact_ids_keeps_polling_open() {
    let rows = [reservation(
        15,
        known_request(115),
        PhysicalRunnerJitState::Started,
        Some(runner(215)),
    )];
    let plan = plan_assigned_population(10, &connection(), 17, &rows, &[], &[]);
    assert_eq!(plan.owned_coverage, 1);
    assert_eq!(plan.actions, []);
    assert_eq!(plan.uncovered, 9);
    assert_eq!(plan.blocked, None);
}

#[test]
fn remote_runner_row_joins_to_its_exact_request_witness_without_starting_another_request() {
    let mut row = reservation(
        8,
        remote_runner("nonce-8"),
        PhysicalRunnerJitState::Started,
        Some(runner(208)),
    );
    row.runner_request_id = Some(108);
    row.started_request_id = Some(108);
    let plan = plan_assigned_population(2, &connection(), 17, &[row.clone()], &[request(109)], &[]);
    assert_eq!(plan.owned_coverage, 1);
    assert_eq!(plan.actions, []);
    assert_eq!(plan.uncovered, 1);
    assert_eq!(plan.blocked, Some(AssignedPopulationBlock::ClosedUnproven));
}

#[test]
fn actual_started_request_can_differ_from_the_acquire_witness() {
    let mut row = reservation(
        9,
        remote_runner("nonce-9"),
        PhysicalRunnerJitState::Started,
        Some(runner(209)),
    );
    row.launch_phase = Some(crate::reconcile::LaunchPhase::RunnerStarted);
    row.witness_request_id = Some(109);
    row.runner_request_id = Some(110);
    row.started_request_id = Some(110);
    row.acquired_request = Some(request(109));
    let plan = plan_assigned_population(2, &connection(), 17, &[row.clone()], &[request(109)], &[]);
    assert_eq!(plan.actions, []);
    assert_eq!(plan.owned_coverage, 1);
    assert_eq!(plan.uncovered, 1);
    assert_eq!(plan.blocked, None);

    let mut conflicting_funding_link = row.clone();
    conflicting_funding_link.acquired_request = Some(request(111));
    let plan = plan_assigned_population(
        2,
        &connection(),
        17,
        &[conflicting_funding_link],
        &[request(109)],
        &[],
    );
    assert_eq!(
        plan.blocked,
        Some(AssignedPopulationBlock::UncertainPhysicalWork)
    );

    let mut conflicting_actual_events = row.clone();
    conflicting_actual_events.completed_request_id = Some(112);
    let plan = plan_assigned_population(
        2,
        &connection(),
        17,
        &[conflicting_actual_events],
        &[request(109)],
        &[],
    );
    assert_eq!(
        plan.blocked,
        Some(AssignedPopulationBlock::UncertainPhysicalWork)
    );

    let mut conflicting_intent_event = row;
    conflicting_intent_event.runner_request_id = Some(113);
    let plan = plan_assigned_population(
        2,
        &connection(),
        17,
        &[conflicting_intent_event],
        &[request(109)],
        &[],
    );
    assert_eq!(
        plan.blocked,
        Some(AssignedPopulationBlock::UncertainPhysicalWork)
    );
}

#[test]
fn uncertain_ownership_closes_population_start() {
    let row = reservation(
        5,
        PhysicalRunnerOwnership::Unknown,
        PhysicalRunnerJitState::Ready,
        Some(runner(205)),
    );
    let plan = plan_assigned_population(2, &connection(), 17, &[row], &[request(105)], &[]);
    assert_eq!(plan.actions, []);
    assert_eq!(
        plan.blocked,
        Some(AssignedPopulationBlock::UncertainPhysicalWork)
    );
}

#[test]
fn duplicate_runner_identity_counts_once_and_lower_total_does_not_release_it() {
    let rows = [
        reservation(
            6,
            known_request(106),
            PhysicalRunnerJitState::Started,
            Some(runner(206)),
        ),
        reservation(
            7,
            known_request(107),
            PhysicalRunnerJitState::Ready,
            Some(runner(206)),
        ),
    ];
    let plan = plan_assigned_population(1, &connection(), 17, &rows, &[], &[]);
    assert_eq!(plan.owned_coverage, 1);
    assert_eq!(plan.actions, []);
    assert_eq!(plan.blocked, None);
}

#[test]
fn negative_population_never_becomes_an_unsigned_count() {
    let plan = plan_assigned_population(-1, &connection(), 17, &[], &[request(109)], &[]);
    assert_eq!(plan.assigned_total, 0);
    assert_eq!(plan.uncovered, 0);
    assert_eq!(plan.blocked, Some(AssignedPopulationBlock::InvalidCount));
}
