use super::{AssignedPopulationBlock, AssignedPopulationPlan};
use crate::journal::{PhysicalRunnerJitState, PhysicalRunnerReservation, ReservationState};

pub(super) fn physical_is_uncertain(reservation: &PhysicalRunnerReservation) -> bool {
    reservation.reservation.state == ReservationState::Uncertain
        || reservation.reservation.state == ReservationState::Recovering
        || reservation.intent_state.is_none()
        || reservation.launch_phase.is_none()
        || reservation.intent_state == Some(crate::journal::IntentState::Uncertain)
        || matches!(
            reservation.jit_state,
            PhysicalRunnerJitState::Uncertain
                | PhysicalRunnerJitState::StartUncertain
                | PhysicalRunnerJitState::Unknown
        )
        || matches!(
            reservation.launch_phase,
            Some(
                crate::reconcile::LaunchPhase::AcquireUncertain
                    | crate::reconcile::LaunchPhase::JitUncertain
                    | crate::reconcile::LaunchPhase::DockerUncertain
                    | crate::reconcile::LaunchPhase::AcquireRequested
                    | crate::reconcile::LaunchPhase::JitRequested
                    | crate::reconcile::LaunchPhase::DockerRequested
            )
        )
}

pub(super) fn physical_covers_population(reservation: &PhysicalRunnerReservation) -> bool {
    reservation.reservation.state != ReservationState::Released
        && !reservation.intent_cleanup_proven()
        && !matches!(
            reservation.jit_state,
            PhysicalRunnerJitState::Completed | PhysicalRunnerJitState::Cleaned
        )
        && (reservation.runner.is_some()
            || (reservation.reservation.resume_eligible && reservation.acquired_request.is_none())
            || matches!(
                reservation.jit_state,
                PhysicalRunnerJitState::Claimed
                    | PhysicalRunnerJitState::Bound
                    | PhysicalRunnerJitState::StartRequested
                    | PhysicalRunnerJitState::Ready
                    | PhysicalRunnerJitState::Started
            ))
}

pub(super) fn blocked_plan(
    assigned_total: u64,
    blocked: AssignedPopulationBlock,
) -> AssignedPopulationPlan {
    AssignedPopulationPlan {
        assigned_total,
        owned_coverage: 0,
        actions: Vec::new(),
        uncovered: assigned_total,
        blocked: Some(blocked),
    }
}
