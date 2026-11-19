// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::IntentState;
use crate::journal::connection::reservation::read::ReservationSnapshot;
use crate::journal::connection::storage::read_transaction;
use crate::journal::connection::{AssignmentIdentity, ConnectionJournal};
use crate::reconcile::LaunchPhase;
use velnor_runner_github::RunnerReference;

mod parse;

const MAX_PHYSICAL_RUNNER_RESERVATIONS: u32 = 4_096;

const PHYSICAL_RUNNER_RESERVATIONS_QUERY: &str = "SELECT r.id, r.intent_id, r.assignment_key, r.state, r.binding_digest, r.sample_generation_id, r.cpu_millicores, r.memory_bytes, r.remote_cleanup_proven, r.local_cleanup_proven, i.id, i.kind, i.assignment_key, i.state, i.cleanup_proven, i.worker_cleanup_proven, i.scale_set_id, i.github_runner_id, i.runner_name, i.runner_request_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.docker_id, i.dind_id, i.worker_volume, i.launch_phase, j.intent_id, j.scale_set_id, j.witness_request_id, j.requested_name, j.state, j.runner_id, j.runner_name, j.started_request_id, j.completed_request_id, EXISTS (SELECT 1 FROM connection_completion_cleanup AS c WHERE c.intent_id = r.intent_id AND c.connection_id = r.connection_id), attempt.witness_request_id, attempt.scale_set_id, attempt.binding_digest, auth.acquire_state, auth.authority_model, auth.binding_digest, auth.started_event_digest, auth.completed_event_digest FROM capacity_reservations AS r LEFT JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id LEFT JOIN connection_runner_jits AS j ON j.intent_id = i.id AND j.connection_id = i.connection_id LEFT JOIN physical_attempt_current AS current ON current.reservation_id = r.id AND current.intent_id = r.intent_id LEFT JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id LEFT JOIN connection_request_authorizations AS auth ON auth.connection_id = attempt.connection_id AND auth.scale_set_id = attempt.scale_set_id AND auth.request_id = attempt.witness_request_id WHERE r.connection_id = ?1 AND r.state != 'released'";

pub(super) struct PhysicalRunnerReservationPage {
    pub(super) reservations: Vec<PhysicalRunnerReservation>,
    pub(super) has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhysicalRunnerJitState {
    Absent,

    Claimed,

    Uncertain,

    Bound,

    StartRequested,

    StartUncertain,

    Ready,

    Started,

    Completed,

    Cleaned,

    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PhysicalRunnerOwnership {
    Known(AssignmentIdentity),

    Unknown,
}

impl PhysicalRunnerOwnership {
    #[must_use]
    pub(crate) fn assignment(&self) -> Option<&AssignmentIdentity> {
        match self {
            Self::Known(identity) => Some(identity),
            Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PhysicalRunnerCleanupEvidence {
    intent: bool,
    worker: bool,
    resources: PhysicalRunnerResourceCleanupEvidence,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PhysicalRunnerResourceCleanupEvidence {
    remote: bool,
    local: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PhysicalRunnerReservation {
    pub(crate) reservation_id: i64,

    pub(crate) intent_id: i64,

    pub(crate) ownership: PhysicalRunnerOwnership,

    pub(crate) reservation: ReservationSnapshot,

    pub(crate) intent_state: Option<IntentState>,

    pub(crate) launch_phase: Option<LaunchPhase>,

    pub(crate) jit_state: PhysicalRunnerJitState,

    pub(crate) requested_runner_name: Option<String>,

    pub(crate) runner: Option<RunnerReference>,

    pub(crate) witness_request_id: Option<i64>,

    pub(crate) started_request_id: Option<i64>,

    pub(crate) completed_request_id: Option<i64>,

    pub(crate) runner_request_id: Option<i64>,

    pub(crate) acquired_request: Option<AssignmentIdentity>,

    pub(crate) worker_volume: Option<String>,

    pub(crate) runner_container_id: Option<String>,

    pub(crate) dind_container_id: Option<String>,

    pub(crate) cleanup_evidence: PhysicalRunnerCleanupEvidence,

    pub(crate) completion_cleanup_pending: bool,
}

impl PhysicalRunnerReservation {
    #[must_use]
    pub(crate) const fn intent_cleanup_proven(&self) -> bool {
        self.cleanup_evidence.intent
    }

    #[must_use]
    pub(crate) const fn worker_cleanup_proven(&self) -> bool {
        self.cleanup_evidence.worker
    }

    #[must_use]
    pub(crate) const fn remote_cleanup_proven(&self) -> bool {
        self.cleanup_evidence.resources.remote
    }

    #[must_use]
    pub(crate) const fn local_cleanup_proven(&self) -> bool {
        self.cleanup_evidence.resources.local
    }
}

impl ConnectionJournal {
    pub(crate) async fn physical_runner_reservations(
        &self,
    ) -> Result<Vec<PhysicalRunnerReservation>, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let current_set_id = parse::current_set_id(connection, self.connection_id().as_str()).await?;
            let mut rows = connection
                .query(
                    "SELECT r.id, r.intent_id, r.assignment_key, r.state, r.binding_digest, r.sample_generation_id, r.cpu_millicores, r.memory_bytes, r.remote_cleanup_proven, r.local_cleanup_proven, i.id, i.kind, i.assignment_key, i.state, i.cleanup_proven, i.worker_cleanup_proven, i.scale_set_id, i.github_runner_id, i.runner_name, i.runner_request_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.docker_id, i.dind_id, i.worker_volume, i.launch_phase, j.intent_id, j.scale_set_id, j.witness_request_id, j.requested_name, j.state, j.runner_id, j.runner_name, j.started_request_id, j.completed_request_id, EXISTS (SELECT 1 FROM connection_completion_cleanup AS c WHERE c.intent_id = r.intent_id AND c.connection_id = r.connection_id), attempt.witness_request_id, attempt.scale_set_id, attempt.binding_digest, auth.acquire_state, auth.authority_model, auth.binding_digest, auth.started_event_digest, auth.completed_event_digest FROM capacity_reservations AS r LEFT JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id LEFT JOIN connection_runner_jits AS j ON j.intent_id = i.id AND j.connection_id = i.connection_id LEFT JOIN physical_attempt_current AS current ON current.reservation_id = r.id AND current.intent_id = r.intent_id LEFT JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id LEFT JOIN connection_request_authorizations AS auth ON auth.connection_id = attempt.connection_id AND auth.scale_set_id = attempt.scale_set_id AND auth.request_id = attempt.witness_request_id WHERE r.connection_id = ?1 AND r.state != 'released' ORDER BY r.id LIMIT ?2",
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

    pub(super) async fn physical_runner_reservations_page_for_worker_health(
        &self,
        after_reservation_id: i64,
        page_size: u32,
    ) -> Result<PhysicalRunnerReservationPage, HostError> {
        if page_size == 0 || page_size > 64 {
            return Err(HostError::Journal);
        }
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let current_set_id =
                parse::current_set_id(connection, self.connection_id().as_str()).await?;
            let mut rows = connection
                .query(
                    &format!(
                        "{PHYSICAL_RUNNER_RESERVATIONS_QUERY} AND r.id > ?2 ORDER BY r.id LIMIT ?3"
                    ),
                    (
                        self.connection_id().as_str(),
                        after_reservation_id,
                        i64::from(page_size + 1),
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut reservations = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                if reservations.len() >= page_size as usize {
                    return Ok(PhysicalRunnerReservationPage {
                        reservations,
                        has_more: true,
                    });
                }
                reservations.push(parse::parse_reservation(&row, self, current_set_id)?);
            }
            Ok(PhysicalRunnerReservationPage {
                reservations,
                has_more: false,
            })
        })
        .await
    }
}
