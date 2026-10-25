use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    LocalWorkerExitDisposition, LocalWorkerExitKind, LocalWorkerExitObservation,
    LocalWorkerHealthCandidate,
};
use crate::error::HostError;
use crate::journal::IntentState;
use crate::journal::connection::storage::{in_transaction, read_transaction};
use crate::journal::connection::{
    AssignmentIdentity, ConnectionJournal, PhysicalRunnerJitState, PhysicalRunnerOwnership,
    PhysicalRunnerReservation, ReservationState,
};
use crate::reconcile::LaunchPhase;

const MAX_LOCAL_WORKER_EXITS: u32 = 4_096;
const HEALTH_CANDIDATE_PAGE_SIZE: u32 = 64;

const CANDIDATE_QUERY: &str = "SELECT attempt.lineage_id, attempt.generation, attempt.physical_nonce, attempt.binding_digest, attempt.engine_id, attempt.docker_root_digest FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id JOIN capacity_reservations AS reservation ON reservation.id = attempt.reservation_id AND reservation.intent_id = attempt.intent_id AND reservation.connection_id = attempt.connection_id JOIN connection_intents AS intent ON intent.id = attempt.intent_id AND intent.connection_id = attempt.connection_id JOIN connection_runtime_bindings AS runtime ON runtime.connection_id = attempt.connection_id AND runtime.scale_set_id = attempt.scale_set_id JOIN connection_runner_jits AS jit ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id AND jit.scale_set_id = attempt.scale_set_id JOIN connection_request_authorizations AS witness ON witness.connection_id = attempt.connection_id AND witness.scale_set_id = attempt.scale_set_id AND witness.request_id = attempt.witness_request_id WHERE current.reservation_id = ?1 AND current.intent_id = ?2 AND attempt.connection_id = ?3 AND attempt.scale_set_id = ?4 AND intent.scale_set_id = attempt.scale_set_id AND attempt.witness_request_id = ?5 AND attempt.binding_digest = ?6 AND reservation.binding_digest = attempt.binding_digest AND reservation.engine_id = attempt.engine_id AND reservation.docker_root_digest = attempt.docker_root_digest AND reservation.assignment_key = ?7 AND intent.assignment_key = ?7 AND (intent.runner_request_id IS NULL OR intent.runner_request_id = jit.started_request_id) AND intent.docker_id = ?8 AND witness.acquire_state = 'acquired' AND witness.authority_model = 'created_empty_exclusive_admin_session_v1' AND witness.binding_digest = attempt.binding_digest AND witness.completed_event_digest IS NULL AND reservation.state IN ('reserved', 'uncertain', 'recovering') AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND intent.kind = 'launch' AND intent.state IN ('pending', 'uncertain', 'done') AND intent.cleanup_proven = 0 AND intent.worker_cleanup_proven = 0 AND intent.runner_completed = 0 AND jit.witness_request_id = attempt.witness_request_id AND jit.completed_request_id IS NULL AND ((jit.state = 'ready' AND intent.launch_phase = 'worker_ready') OR (jit.state = 'started' AND intent.launch_phase = 'runner_started')) AND NOT EXISTS (SELECT 1 FROM connection_request_authorizations AS completed WHERE completed.connection_id = attempt.connection_id AND completed.scale_set_id = attempt.scale_set_id AND completed.completed_intent_id = attempt.intent_id AND completed.completed_event_digest IS NOT NULL)  AND NOT EXISTS (SELECT 1 FROM local_worker_exits AS exit WHERE exit.lineage_id = attempt.lineage_id AND exit.generation = attempt.generation)";

const CURRENT_CANDIDATE_QUERY: &str = "SELECT 1 FROM physical_attempt_current AS current JOIN physical_attempts AS attempt ON attempt.lineage_id = current.lineage_id AND attempt.generation = current.generation AND attempt.reservation_id = current.reservation_id AND attempt.intent_id = current.intent_id JOIN capacity_reservations AS reservation ON reservation.id = attempt.reservation_id AND reservation.intent_id = attempt.intent_id AND reservation.connection_id = attempt.connection_id JOIN connection_intents AS intent ON intent.id = attempt.intent_id AND intent.connection_id = attempt.connection_id JOIN connection_runtime_bindings AS runtime ON runtime.connection_id = attempt.connection_id AND runtime.scale_set_id = attempt.scale_set_id JOIN connection_runner_jits AS jit ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id AND jit.scale_set_id = attempt.scale_set_id JOIN connection_request_authorizations AS witness ON witness.connection_id = attempt.connection_id AND witness.scale_set_id = attempt.scale_set_id AND witness.request_id = attempt.witness_request_id WHERE current.lineage_id = ?1 AND current.generation = ?2 AND current.reservation_id = ?3 AND current.intent_id = ?4 AND attempt.connection_id = ?5 AND attempt.scale_set_id = ?6 AND intent.scale_set_id = attempt.scale_set_id AND attempt.witness_request_id = ?7 AND attempt.physical_nonce = ?8 AND attempt.binding_digest = ?9 AND attempt.engine_id = ?10 AND attempt.docker_root_digest = ?11 AND reservation.binding_digest = attempt.binding_digest AND reservation.engine_id = attempt.engine_id AND reservation.docker_root_digest = attempt.docker_root_digest AND reservation.assignment_key = ?12 AND intent.assignment_key = ?12 AND (intent.runner_request_id IS NULL OR intent.runner_request_id = jit.started_request_id) AND intent.docker_id = ?13 AND witness.acquire_state = 'acquired' AND witness.authority_model = 'created_empty_exclusive_admin_session_v1' AND witness.binding_digest = attempt.binding_digest AND witness.completed_event_digest IS NULL AND reservation.state IN ('reserved', 'uncertain', 'recovering') AND reservation.remote_cleanup_proven = 0 AND reservation.local_cleanup_proven = 0 AND intent.kind = 'launch' AND intent.state IN ('pending', 'uncertain', 'done') AND intent.cleanup_proven = 0 AND intent.worker_cleanup_proven = 0 AND intent.runner_completed = 0 AND jit.witness_request_id = attempt.witness_request_id AND jit.completed_request_id IS NULL AND ((jit.state = 'ready' AND intent.launch_phase = 'worker_ready') OR (jit.state = 'started' AND intent.launch_phase = 'runner_started')) AND NOT EXISTS (SELECT 1 FROM connection_request_authorizations AS completed WHERE completed.connection_id = attempt.connection_id AND completed.scale_set_id = attempt.scale_set_id AND completed.completed_intent_id = attempt.intent_id AND completed.completed_event_digest IS NOT NULL) ";

const EXISTING_EXIT_QUERY: &str = "SELECT connection_id, scale_set_id, reservation_id, intent_id, binding_digest, engine_id, docker_root_digest, runner_container_id FROM local_worker_exits WHERE lineage_id = ?1 AND generation = ?2";

const UNRESOLVED_EXITS_QUERY: &str = "SELECT exit.lineage_id, exit.generation, exit.reservation_id, exit.intent_id, exit.scale_set_id, exit.runner_container_id, exit.kind, exit.exit_code, exit.observed_at_unix_ms FROM local_worker_exits AS exit JOIN capacity_reservations AS reservation ON reservation.id = exit.reservation_id AND reservation.intent_id = exit.intent_id AND reservation.connection_id = exit.connection_id JOIN connection_intents AS intent ON intent.id = exit.intent_id AND intent.connection_id = exit.connection_id LEFT JOIN connection_runner_jits AS jit ON jit.intent_id = exit.intent_id AND jit.connection_id = exit.connection_id WHERE exit.connection_id = ?1 AND NOT (reservation.state = 'released' AND reservation.remote_cleanup_proven = 1 AND reservation.local_cleanup_proven = 1 AND intent.cleanup_proven = 1 AND COALESCE(jit.state, '') = 'cleaned') ORDER BY exit.observed_at_unix_ms, exit.lineage_id, exit.generation LIMIT ?2";

impl LocalWorkerHealthCandidate {
    pub(crate) const fn lineage_id(&self) -> i64 {
        self.lineage_id
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
}

impl LocalWorkerExitObservation {
    pub(crate) const fn lineage_id(&self) -> i64 {
        self.lineage_id
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn intent_id(&self) -> i64 {
        self.intent_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) const fn kind(&self) -> LocalWorkerExitKind {
        self.kind
    }
}

type CandidateSeed = (i64, i64, i64, i64, AssignmentIdentity, String);

impl ConnectionJournal {
    pub(crate) async fn local_worker_health_candidate_page_after(
        &self,
        after_reservation_id: i64,
    ) -> Result<(Vec<LocalWorkerHealthCandidate>, Option<i64>, bool), HostError> {
        let page = self
            .physical_runner_reservations_page_for_worker_health(
                after_reservation_id,
                HEALTH_CANDIDATE_PAGE_SIZE,
            )
            .await?;
        let scanned_through = page.reservations.last().map(|row| row.reservation_id);
        let seeds = page
            .reservations
            .iter()
            .filter_map(|reservation| candidate_seed(self, reservation))
            .collect::<Vec<_>>();
        let candidates = read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut candidates = Vec::new();
            for seed in &seeds {
                if let Some(candidate) = candidate_from_seed(connection, self, seed).await? {
                    candidates.push(candidate);
                }
            }
            Ok(candidates)
        })
        .await?;
        Ok((candidates, scanned_through, page.has_more))
    }

    pub(crate) async fn record_local_worker_exit(
        &self,
        candidate: &LocalWorkerHealthCandidate,
        kind: LocalWorkerExitKind,
    ) -> Result<LocalWorkerExitDisposition, HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            if candidate.connection_id != self.connection_id().as_str()
                || candidate.binding_digest != self.binding_digest()
                || candidate.assignment.connection_id() != self.connection_id()
                || candidate.assignment.scale_set_id() != candidate.scale_set_id
            {
                return Err(HostError::Journal);
            }
            if existing_exit(connection, candidate).await? {
                return Ok(LocalWorkerExitDisposition::AlreadyRecorded);
            }
            if !current_candidate_matches(connection, self, candidate).await? {
                return Ok(LocalWorkerExitDisposition::IgnoredStale);
            }
            let observed_at_unix_ms = unix_time_millis()?;
            let (kind, exit_code) = match kind {
                LocalWorkerExitKind::Exited { exit_code } => {
                    ("exited", Some(i64::from(exit_code)))
                }
                LocalWorkerExitKind::Missing => ("missing", None),
            };
            connection
                .execute(
                    "INSERT INTO local_worker_exits (lineage_id, generation, reservation_id, intent_id, connection_id, scale_set_id, binding_digest, engine_id, docker_root_digest, runner_container_id, kind, exit_code, observed_at_unix_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    (
                        candidate.lineage_id,
                        candidate.generation,
                        candidate.reservation_id,
                        candidate.intent_id,
                        candidate.connection_id.as_str(),
                        candidate.scale_set_id,
                        candidate.binding_digest.as_str(),
                        candidate.engine_id.as_str(),
                        candidate.docker_root_digest.as_str(),
                        candidate.runner_container_id.as_str(),
                        kind,
                        exit_code,
                        observed_at_unix_ms,
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(LocalWorkerExitDisposition::Recorded)
        })
        .await
    }

    pub(crate) async fn unresolved_local_worker_exits(
        &self,
    ) -> Result<Vec<LocalWorkerExitObservation>, HostError> {
        read_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    UNRESOLVED_EXITS_QUERY,
                    (
                        self.connection_id().as_str(),
                        i64::from(MAX_LOCAL_WORKER_EXITS + 1),
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut exits = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                if exits.len() >= MAX_LOCAL_WORKER_EXITS as usize {
                    return Err(HostError::Journal);
                }
                exits.push(parse_observation(&row)?);
            }
            Ok(exits)
        })
        .await
    }
}

fn candidate_seed(
    scope: &ConnectionJournal,
    reservation: &PhysicalRunnerReservation,
) -> Option<CandidateSeed> {
    let launch_phase = match reservation.jit_state {
        PhysicalRunnerJitState::Ready => LaunchPhase::WorkerReady,
        PhysicalRunnerJitState::Started => LaunchPhase::RunnerStarted,
        _ => return None,
    };
    if reservation.reservation.state == ReservationState::Released
        || reservation
            .intent_state
            .is_none_or(|state| state == IntentState::Failed)
        || reservation.launch_phase != Some(launch_phase)
        || reservation.intent_cleanup_proven()
        || reservation.worker_cleanup_proven()
        || reservation.remote_cleanup_proven()
        || reservation.local_cleanup_proven()
        || reservation.completion_cleanup_pending
    {
        return None;
    }
    let PhysicalRunnerOwnership::Known(assignment) = &reservation.ownership else {
        return None;
    };
    let witness_request_id = reservation.witness_request_id?;
    if assignment.connection_id() != scope.connection_id() || assignment.scale_set_id() <= 0 {
        return None;
    }
    Some((
        reservation.reservation_id,
        reservation.intent_id,
        witness_request_id,
        assignment.scale_set_id(),
        assignment.clone(),
        reservation.runner_container_id.clone()?,
    ))
}

async fn candidate_from_seed(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    seed: &CandidateSeed,
) -> Result<Option<LocalWorkerHealthCandidate>, HostError> {
    let (
        reservation_id,
        intent_id,
        witness_request_id,
        scale_set_id,
        assignment,
        runner_container_id,
    ) = seed;
    let assignment_key = assignment.storage_key();
    let mut rows = connection
        .query(
            CANDIDATE_QUERY,
            (
                *reservation_id,
                *intent_id,
                scope.connection_id().as_str(),
                *scale_set_id,
                *witness_request_id,
                scope.binding_digest(),
                assignment_key.as_str(),
                runner_container_id.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let candidate = LocalWorkerHealthCandidate {
        connection_id: scope.connection_id().as_str().to_owned(),
        scale_set_id: *scale_set_id,
        assignment: assignment.clone(),
        lineage_id: row.get(0).map_err(|_| HostError::Journal)?,
        generation: row.get(1).map_err(|_| HostError::Journal)?,
        reservation_id: *reservation_id,
        intent_id: *intent_id,
        witness_request_id: *witness_request_id,
        physical_nonce: row.get(2).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(3).map_err(|_| HostError::Journal)?,
        engine_id: row.get(4).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(5).map_err(|_| HostError::Journal)?,
        runner_container_id: runner_container_id.clone(),
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(candidate))
}

async fn existing_exit(
    connection: &turso::Connection,
    candidate: &LocalWorkerHealthCandidate,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            EXISTING_EXIT_QUERY,
            (candidate.lineage_id, candidate.generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let matches = row.get::<String>(0).map_err(|_| HostError::Journal)? == candidate.connection_id
        && row.get::<i64>(1).map_err(|_| HostError::Journal)? == candidate.scale_set_id
        && row.get::<i64>(2).map_err(|_| HostError::Journal)? == candidate.reservation_id
        && row.get::<i64>(3).map_err(|_| HostError::Journal)? == candidate.intent_id
        && row.get::<String>(4).map_err(|_| HostError::Journal)? == candidate.binding_digest
        && row.get::<String>(5).map_err(|_| HostError::Journal)? == candidate.engine_id
        && row.get::<String>(6).map_err(|_| HostError::Journal)? == candidate.docker_root_digest
        && row.get::<String>(7).map_err(|_| HostError::Journal)? == candidate.runner_container_id;
    if !matches || rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(true)
}

async fn current_candidate_matches(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    candidate: &LocalWorkerHealthCandidate,
) -> Result<bool, HostError> {
    let assignment_key = candidate.assignment.storage_key();
    let mut rows = connection
        .query(
            CURRENT_CANDIDATE_QUERY,
            (
                candidate.lineage_id,
                candidate.generation,
                candidate.reservation_id,
                candidate.intent_id,
                scope.connection_id().as_str(),
                candidate.scale_set_id,
                candidate.witness_request_id,
                candidate.physical_nonce.as_str(),
                candidate.binding_digest.as_str(),
                candidate.engine_id.as_str(),
                candidate.docker_root_digest.as_str(),
                assignment_key.as_str(),
                candidate.runner_container_id.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let matches = rows.next().await.map_err(|_| HostError::Journal)?.is_some();
    if matches && rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(matches)
}

fn parse_observation(row: &turso::Row) -> Result<LocalWorkerExitObservation, HostError> {
    let observation_kind = row.get::<String>(6).map_err(|_| HostError::Journal)?;
    let exit_code = row.get::<Option<i64>>(7).map_err(|_| HostError::Journal)?;
    let kind = match (observation_kind.as_str(), exit_code) {
        ("exited", Some(exit_code)) => LocalWorkerExitKind::Exited {
            exit_code: i32::try_from(exit_code).map_err(|_| HostError::Journal)?,
        },
        ("missing", None) => LocalWorkerExitKind::Missing,
        _ => return Err(HostError::Journal),
    };
    let observed_at_unix_ms = u64::try_from(row.get::<i64>(8).map_err(|_| HostError::Journal)?)
        .map_err(|_| HostError::Journal)?;
    Ok(LocalWorkerExitObservation {
        lineage_id: row.get(0).map_err(|_| HostError::Journal)?,
        generation: row.get(1).map_err(|_| HostError::Journal)?,
        reservation_id: row.get(2).map_err(|_| HostError::Journal)?,
        intent_id: row.get(3).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(4).map_err(|_| HostError::Journal)?,
        runner_container_id: row.get(5).map_err(|_| HostError::Journal)?,
        kind,
        observed_at_unix_ms,
    })
}

fn unix_time_millis() -> Result<i64, HostError> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_millis(),
    )
    .map_err(|_| HostError::Journal)
}

#[cfg(test)]
#[path = "worker_health/tests.rs"]
mod tests;
