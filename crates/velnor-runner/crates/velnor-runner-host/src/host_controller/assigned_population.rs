//! Plan work from the assigned job count in the Scale Set response.

use std::collections::{BTreeMap, BTreeSet};

#[path = "assigned_population/coverage.rs"]
mod coverage;
use coverage::{blocked_plan, physical_covers_population, physical_is_uncertain};

use crate::config::ConnectionId;
use crate::journal::{
    AcquiredRecoveryCandidate, AssignmentIdentity, AssignmentKey, PhysicalRunnerJitState,
    PhysicalRunnerOwnership, PhysicalRunnerReservation, ReservationState,
};

#[cfg(test)]
#[path = "assigned_population_tests.rs"]
mod tests;

/// Work that an acquired request with source approval can support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssignedPopulationAction {
    /// Continue an existing reservation through the checked recovery path.
    ContinueExisting {
        /// Request with a saved `AcquireJobs` result.
        request: AssignmentIdentity,
        /// Existing reservation. Do not reserve it again.
        reservation_id: i64,
        /// Index of the recovery candidate checked against the journal.
        candidate_index: usize,
    },
    /// This action reserves a successor for acquired demand after a terminal attempt is proven.
    ReserveSuccessor {
        /// This request was acquired earlier. The method does not call `AcquireJobs` again.
        request: AssignmentIdentity,
    },
}

/// Reason that the planner cannot safely plan more work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssignedPopulationBlock {
    /// The assigned job count or Scale Set ID is outside the allowed range.
    InvalidCount,
    /// A reservation row has unknown or uncertain ownership.
    UncertainPhysicalWork,
    /// The journal has more than one reservation for one request.
    DuplicateRequestReservation,
    /// A runner has no linked acquired request.
    MissingSourceWitness,
    /// An acquired request has no saved reservation with the same identity.
    ClosedUnproven,
}

/// Plan from the assigned job count and saved reservations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssignedPopulationPlan {
    /// Full assigned count from `Statistics.TotalAssignedJobs`.
    pub(crate) assigned_total: u64,
    /// Separate healthy runners or known preparation rows for this Scale Set.
    pub(crate) owned_coverage: u64,
    /// Each action names a request and reservation from the journal. This list has no generated request IDs.
    pub(crate) actions: Vec<AssignedPopulationAction>,
    /// Demand without an acquired request with the same identity.
    pub(crate) uncovered: u64,
    /// Reason to stop when source or reservation state prevents planning.
    pub(crate) blocked: Option<AssignedPopulationBlock>,
}

struct PhysicalCoverage {
    count: u64,
    request_reservations: BTreeMap<i64, Vec<(i64, bool, bool)>>,
    has_unattributed_runner: bool,
}

/// Compare assigned demand with all saved reservations and `AcquireJobs` results.
///
/// The caller checks that the session is current. The caller sets the capacity header. The caller uses the resource permit.
/// This function does not call `AcquireJobs`. It does not create a reservation.
pub(crate) fn plan_assigned_population(
    assigned_total: i64,
    connection_id: &ConnectionId,
    scale_set_id: i64,
    reservations: &[PhysicalRunnerReservation],
    acquired: &[AssignmentIdentity],
    recovery_candidates: &[AcquiredRecoveryCandidate],
) -> AssignedPopulationPlan {
    plan_assigned_population_with_successors(
        assigned_total,
        connection_id,
        scale_set_id,
        reservations,
        acquired,
        recovery_candidates,
        &[],
    )
}

pub(crate) fn plan_assigned_population_with_successors(
    assigned_total: i64,
    connection_id: &ConnectionId,
    scale_set_id: i64,
    reservations: &[PhysicalRunnerReservation],
    acquired: &[AssignmentIdentity],
    recovery_candidates: &[AcquiredRecoveryCandidate],
    successor_ready: &[AssignmentIdentity],
) -> AssignedPopulationPlan {
    let Ok(assigned_total) = u64::try_from(assigned_total) else {
        return blocked_plan(0, AssignedPopulationBlock::InvalidCount);
    };
    if scale_set_id <= 0 {
        return blocked_plan(assigned_total, AssignedPopulationBlock::InvalidCount);
    }

    let coverage = match collect_physical_coverage(
        connection_id,
        scale_set_id,
        reservations,
        recovery_candidates,
    ) {
        Ok(coverage) => coverage,
        Err(blocked) => return blocked_plan(assigned_total, blocked),
    };
    let owned_coverage = coverage.count;

    let shortfall = assigned_total.saturating_sub(owned_coverage);
    if coverage.has_unattributed_runner {
        return AssignedPopulationPlan {
            assigned_total,
            owned_coverage,
            actions: Vec::new(),
            uncovered: shortfall,
            blocked: Some(AssignedPopulationBlock::MissingSourceWitness),
        };
    }
    let (actions, blocked) = select_acquired_actions(
        connection_id,
        scale_set_id,
        acquired,
        recovery_candidates,
        &coverage.request_reservations,
        successor_ready,
        shortfall,
    );
    let uncovered = shortfall.saturating_sub(actions.len() as u64);
    AssignedPopulationPlan {
        assigned_total,
        owned_coverage,
        actions,
        uncovered,
        blocked,
    }
}

fn collect_physical_coverage(
    connection_id: &ConnectionId,
    scale_set_id: i64,
    reservations: &[PhysicalRunnerReservation],
    recovery_candidates: &[AcquiredRecoveryCandidate],
) -> Result<PhysicalCoverage, AssignedPopulationBlock> {
    let mut physical_ids = BTreeSet::new();
    let mut request_reservations: BTreeMap<i64, Vec<(i64, bool, bool)>> = BTreeMap::new();
    let mut runner_ids = BTreeSet::new();
    let mut count = 0_u64;
    let mut has_unattributed_runner = false;
    for reservation in reservations {
        if !physical_ids.insert(reservation.reservation_id) || physical_is_uncertain(reservation) {
            return Err(AssignedPopulationBlock::UncertainPhysicalWork);
        }
        let PhysicalRunnerOwnership::Known(identity) = &reservation.ownership else {
            return Err(AssignedPopulationBlock::UncertainPhysicalWork);
        };
        if identity.connection_id() != connection_id {
            return Err(AssignedPopulationBlock::UncertainPhysicalWork);
        }
        if identity.scale_set_id() != scale_set_id {
            continue;
        }
        let covers = physical_covers_population(reservation);
        let candidate_matches = recovery_candidates.iter().any(|candidate| {
            candidate.reservation_id() == reservation.reservation_id
                && reservation.acquired_request.as_ref() == Some(candidate.request())
        });
        let exact_acquired_link = reservation
            .acquired_request
            .as_ref()
            .is_some_and(|request| {
                reservation.reservation.state == ReservationState::Reserved
                    && reservation.intent_state == Some(crate::journal::IntentState::Pending)
                    && reservation.launch_phase
                        == Some(crate::reconcile::LaunchPhase::ProvisioningHeld)
                    && reservation.jit_state == PhysicalRunnerJitState::Absent
                    && request.request_id().is_some()
            });
        if !covers && !candidate_matches && !exact_acquired_link {
            return Err(AssignedPopulationBlock::UncertainPhysicalWork);
        }
        let request_id = reservation_request_id(reservation, identity)?;
        if let Some(request_id) = request_id {
            request_reservations.entry(request_id).or_default().push((
                reservation.reservation_id,
                covers,
                exact_acquired_link,
            ));
        } else if covers {
            has_unattributed_runner = true;
        }
        if covers && insert_runner_identity(reservation, &mut runner_ids) {
            count = count.saturating_add(1);
        }
    }
    Ok(PhysicalCoverage {
        count,
        request_reservations,
        has_unattributed_runner,
    })
}

fn reservation_request_id(
    reservation: &PhysicalRunnerReservation,
    identity: &AssignmentIdentity,
) -> Result<Option<i64>, AssignedPopulationBlock> {
    let actual_ids = [
        reservation.runner_request_id,
        reservation.started_request_id,
        reservation.completed_request_id,
    ];
    if actual_ids.into_iter().flatten().any(|value| value <= 0)
        || (reservation.started_request_id.is_some()
            && reservation.completed_request_id.is_some()
            && reservation.started_request_id != reservation.completed_request_id)
        || reservation.runner_request_id.is_some_and(|actual| {
            reservation.started_request_id != Some(actual)
                && reservation.completed_request_id != Some(actual)
        })
        || reservation
            .witness_request_id
            .is_some_and(|witness| witness <= 0)
    {
        return Err(AssignedPopulationBlock::UncertainPhysicalWork);
    }
    let linked_request_id = reservation
        .acquired_request
        .as_ref()
        .and_then(AssignmentIdentity::request_id);
    if reservation
        .witness_request_id
        .is_some_and(|witness| linked_request_id.is_some_and(|linked| linked != witness))
    {
        return Err(AssignedPopulationBlock::UncertainPhysicalWork);
    }
    let request_id = linked_request_id
        .or(reservation.witness_request_id)
        .or(reservation.runner_request_id)
        .or(reservation.started_request_id)
        .or(reservation.completed_request_id)
        .or(identity.request_id());
    if let Some(request_id) = request_id {
        AssignmentIdentity::new(
            identity.connection_id(),
            identity.scale_set_id(),
            AssignmentKey::AssignedRequest { request_id },
        )
        .map_err(|_| AssignedPopulationBlock::UncertainPhysicalWork)?;
    }
    Ok(request_id)
}

fn insert_runner_identity(
    reservation: &PhysicalRunnerReservation,
    runner_ids: &mut BTreeSet<i64>,
) -> bool {
    match reservation.runner.as_ref() {
        Some(runner) => runner_ids.insert(runner.id),
        None => runner_ids.insert(-reservation.reservation_id),
    }
}

fn select_acquired_actions(
    connection_id: &ConnectionId,
    scale_set_id: i64,
    acquired: &[AssignmentIdentity],
    recovery_candidates: &[AcquiredRecoveryCandidate],
    request_reservations: &BTreeMap<i64, Vec<(i64, bool, bool)>>,
    successor_ready: &[AssignmentIdentity],
    shortfall: u64,
) -> (
    Vec<AssignedPopulationAction>,
    Option<AssignedPopulationBlock>,
) {
    let mut actions = Vec::new();
    let mut seen_request_ids = BTreeSet::new();
    for identity in acquired {
        let Some(request_id) = identity.request_id() else {
            continue;
        };
        if identity.connection_id() != connection_id || identity.scale_set_id() != scale_set_id {
            continue;
        }
        if !seen_request_ids.insert(request_id) {
            continue;
        }
        let reservations = request_reservations.get(&request_id);
        let matching_candidate = reservations.and_then(|reservations| {
            recovery_candidates
                .iter()
                .enumerate()
                .find(|(_, candidate)| {
                    candidate.request() == identity
                        && reservations
                            .iter()
                            .any(|(id, _, _)| *id == candidate.reservation_id())
                })
        });
        if let Some((candidate_index, candidate)) = matching_candidate {
            if actions.len() < usize::try_from(shortfall).unwrap_or(usize::MAX) {
                actions.push(AssignedPopulationAction::ContinueExisting {
                    request: identity.clone(),
                    reservation_id: candidate.reservation_id(),
                    candidate_index,
                });
            }
            continue;
        }
        if successor_ready.contains(identity) {
            if actions.len() < usize::try_from(shortfall).unwrap_or(usize::MAX) {
                actions.push(AssignedPopulationAction::ReserveSuccessor {
                    request: identity.clone(),
                });
            }
            continue;
        }
        let Some(reservations) = reservations else {
            return (Vec::new(), Some(AssignedPopulationBlock::ClosedUnproven));
        };
        if !reservations
            .iter()
            .any(|(_, covered, exact_link)| *covered || *exact_link)
        {
            return (Vec::new(), Some(AssignedPopulationBlock::ClosedUnproven));
        }
    }
    for candidate in recovery_candidates {
        let Some(request_id) = candidate.request().request_id() else {
            return (
                Vec::new(),
                Some(AssignedPopulationBlock::UncertainPhysicalWork),
            );
        };
        if !seen_request_ids.contains(&request_id)
            || request_reservations.get(&request_id).is_none_or(|rows| {
                !rows
                    .iter()
                    .any(|(id, _, _)| *id == candidate.reservation_id())
            })
        {
            return (
                Vec::new(),
                Some(AssignedPopulationBlock::UncertainPhysicalWork),
            );
        }
    }
    (actions, None)
}
