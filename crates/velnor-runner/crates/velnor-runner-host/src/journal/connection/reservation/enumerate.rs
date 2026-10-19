//! Complete, connection-scoped reads of charged physical runner intents.

use crate::error::HostError;
use crate::journal::IntentState;
use crate::journal::connection::reservation::read::ReservationSnapshot;
use crate::journal::connection::storage::read_transaction;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};
use crate::reconcile::LaunchPhase;
use velnor_runner_github::RunnerReference;

mod parse;

const MAX_PHYSICAL_RUNNER_RESERVATIONS: u32 = 4_096;

/// Durable state of the one-shot GitHub runner JIT operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhysicalRunnerJitState {
    /// The physical reservation has not claimed JIT.
    Absent,
    /// The JIT request was claimed before its HTTP effect.
    Claimed,
    /// The JIT effect may have completed without a recorded runner identity.
    Uncertain,
    /// The exact JIT runner identity was recorded.
    Bound,
    /// Docker start was claimed.
    StartRequested,
    /// Docker start may have completed without an exact response.
    StartUncertain,
    /// The worker pair was created and inspected.
    Ready,
    /// A matching Started event was recorded.
    Started,
    /// A matching Completed event was recorded.
    Completed,
    /// Proven cleanup finished for the JIT runner.
    Cleaned,
    /// A persisted JIT state or its related identity is not recognized.
    Unknown,
}

/// Whether durable rows prove one exact physical runner assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PhysicalRunnerOwnership {
    /// Exact connection, Scale Set, and physical nonce assignment.
    Known(AssignmentIdentity),
    /// Row data is incomplete or inconsistent; its charge remains visible.
    Unknown,
}

impl PhysicalRunnerOwnership {
    /// Give the exact assignment only when all stored identity fields agree.
    #[must_use]
    pub(crate) fn assignment(&self) -> Option<&AssignmentIdentity> {
        match self {
            Self::Known(identity) => Some(identity),
            Self::Unknown => None,
        }
    }
}

/// One charged runner capacity row, including uncertain and cleanup states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PhysicalRunnerReservation {
    /// Stable reservation row ID.
    pub(crate) reservation_id: i64,
    /// Stable launch intent row ID.
    pub(crate) intent_id: i64,
    /// Exact or explicitly unknown assignment ownership.
    pub(crate) ownership: PhysicalRunnerOwnership,
    /// Immutable charge and its current lifecycle state.
    pub(crate) reservation: ReservationSnapshot,
    /// Launch intent state; `None` means the persisted value is unknown.
    pub(crate) intent_state: Option<IntentState>,
    /// Last known launch effect phase.
    pub(crate) launch_phase: Option<LaunchPhase>,
    /// State of the JIT and Docker start operation.
    pub(crate) jit_state: PhysicalRunnerJitState,
    /// Requested unique runner name when JIT has been claimed.
    pub(crate) requested_runner_name: Option<String>,
    /// Exact JIT-created runner when its identity is recorded.
    pub(crate) runner: Option<RunnerReference>,
    /// Request selected as the JIT audit witness.
    pub(crate) witness_request_id: Option<i64>,
    /// Request correlated by a Started event.
    pub(crate) started_request_id: Option<i64>,
    /// Request correlated by a Completed event.
    pub(crate) completed_request_id: Option<i64>,
    /// Request currently joined to the physical runner.
    pub(crate) runner_request_id: Option<i64>,
    /// Exact acquired request linked to this physical reservation.
    pub(crate) acquired_request: Option<AssignmentIdentity>,
    /// Durable worker volume identity.
    pub(crate) worker_volume: Option<String>,
    /// Runner worker container identity.
    pub(crate) runner_container_id: Option<String>,
    /// Private `DinD` container identity.
    pub(crate) dind_container_id: Option<String>,
    /// Whether the intent row itself records cleanup proof.
    pub(crate) intent_cleanup_proven: bool,
    /// Whether worker-volume cleanup has proof.
    pub(crate) worker_cleanup_proven: bool,
    /// Whether remote runner cleanup has proof.
    pub(crate) remote_cleanup_proven: bool,
    /// Whether local container cleanup has proof.
    pub(crate) local_cleanup_proven: bool,
    /// Whether completion still has a cleanup row.
    pub(crate) completion_cleanup_pending: bool,
}

impl ConnectionJournal {
    /// Give every nonreleased capacity row for this repository connection.
    ///
    /// The result includes rows for older Scale Set IDs. Rows whose immutable
    /// connection, Scale Set, assignment, or effect identity disagree stay in
    /// the result as `Unknown` ownership. More than 4096 live rows makes the
    /// complete enumeration fail closed.
    pub(crate) async fn physical_runner_reservations(
        &self,
    ) -> Result<Vec<PhysicalRunnerReservation>, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let current_set_id = parse::current_set_id(connection, self.connection_id().as_str()).await?;
            let mut rows = connection
                .query(
                    "SELECT r.id, r.intent_id, r.assignment_key, r.state, r.binding_digest, r.sample_generation_id, r.cpu_millicores, r.memory_bytes, r.remote_cleanup_proven, r.local_cleanup_proven, i.id, i.kind, i.assignment_key, i.state, i.cleanup_proven, i.worker_cleanup_proven, i.scale_set_id, i.github_runner_id, i.runner_name, i.runner_request_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.docker_id, i.dind_id, i.worker_volume, i.launch_phase, j.intent_id, j.scale_set_id, j.witness_request_id, j.requested_name, j.state, j.runner_id, j.runner_name, j.started_request_id, j.completed_request_id, EXISTS (SELECT 1 FROM connection_completion_cleanup AS c WHERE c.intent_id = r.intent_id AND c.connection_id = r.connection_id), link.request_id, link.scale_set_id, link.binding_digest, auth.acquire_state, auth.authority_model, auth.binding_digest, auth.started_event_digest, auth.completed_event_digest FROM capacity_reservations AS r LEFT JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id LEFT JOIN connection_runner_jits AS j ON j.intent_id = i.id AND j.connection_id = i.connection_id LEFT JOIN connection_request_reservations AS link ON link.reservation_id = r.id AND link.connection_id = r.connection_id LEFT JOIN connection_request_authorizations AS auth ON auth.connection_id = link.connection_id AND auth.scale_set_id = link.scale_set_id AND auth.request_id = link.request_id WHERE r.connection_id = ?1 AND r.state != 'released' ORDER BY r.id LIMIT ?2",
                    (self.connection_id().as_str(), i64::from(MAX_PHYSICAL_RUNNER_RESERVATIONS + 1)),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut result = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                if result.len() >= MAX_PHYSICAL_RUNNER_RESERVATIONS as usize {
                    return Err(HostError::Journal);
                }
                result.push(parse::parse_reservation(
                    &row,
                    self,
                    current_set_id,
                )?);
            }
            Ok(result)
        })
        .await
    }
}
