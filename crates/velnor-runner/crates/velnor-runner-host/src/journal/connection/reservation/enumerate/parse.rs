//! Durable physical-reservation row decoding.

use crate::error::HostError;
use crate::journal::IntentState;
use crate::journal::connection::reservation::read::{
    ReservationSnapshot, ReservationState, parse_state,
};
use crate::journal::connection::{AssignmentIdentity, AssignmentKey, ConnectionJournal};
use crate::reconcile::LaunchPhase;
use velnor_runner_github::RunnerReference;

use super::{PhysicalRunnerJitState, PhysicalRunnerOwnership, PhysicalRunnerReservation};

struct ReservationRow {
    reservation_id: i64,
    intent_id: i64,
    key: String,
    state: ReservationState,
    binding_digest: String,
    generation: Option<String>,
    cpu: u64,
    memory: u64,
    remote_cleanup_proven: bool,
    local_cleanup_proven: bool,
}

struct IntentSnapshot {
    row_id: Option<i64>,
    kind: Option<String>,
    key: Option<String>,
    state: Option<IntentState>,
    cleanup_proven: Option<bool>,
    worker_cleanup_proven: Option<bool>,
    set_id: Option<i64>,
    runner_id: Option<String>,
    runner_name: Option<String>,
    request_id: Option<i64>,
    acquire_attempted: Option<bool>,
    acquire_resolved: Option<bool>,
    acquired: Option<bool>,
    jit_requested: Option<bool>,
    runner_container_id: Option<String>,
    dind_container_id: Option<String>,
    worker_volume: Option<String>,
    phase: Option<LaunchPhase>,
}

struct RunnerJitSnapshot {
    intent_id: Option<i64>,
    set_id: Option<i64>,
    witness_request_id: Option<i64>,
    requested_name: Option<String>,
    state: PhysicalRunnerJitState,
    state_known: bool,
    runner_id: Option<i64>,
    runner_name: Option<String>,
    started_request_id: Option<i64>,
    completed_request_id: Option<i64>,
    completion_cleanup_pending: bool,
}

pub(super) fn parse_reservation(
    row: &turso::Row,
    scope: &ConnectionJournal,
    current_set_id: Option<i64>,
) -> Result<PhysicalRunnerReservation, HostError> {
    let reservation = read_reservation(row)?;
    let intent = read_intent(row)?;
    let jit = read_runner_jit(row)?;
    let encoded_set_id = assignment_set_id(scope, &reservation.key);
    let set_id = intent.set_id.or(encoded_set_id).unwrap_or_default();
    let assignment = parse_assignment(scope, set_id, &reservation.key);
    let runner = parse_runner(&intent, &jit, set_id);
    let expected_name = assignment.as_ref().map(physical_runner_name);
    let known = reservation_identity_is_known(
        &reservation,
        &intent,
        &jit,
        scope,
        set_id,
        current_set_id,
        assignment.is_some(),
        expected_name.as_deref(),
        runner.is_some(),
    );
    let ownership = match (known, assignment) {
        (true, Some(identity)) => PhysicalRunnerOwnership::Known(identity),
        _ => PhysicalRunnerOwnership::Unknown,
    };
    let resume_eligible = resume_is_eligible(&reservation, &intent, &ownership);
    Ok(build_reservation(
        reservation,
        intent,
        jit,
        runner,
        ownership,
        resume_eligible,
    ))
}

fn reservation_identity_is_known(
    reservation: &ReservationRow,
    intent: &IntentSnapshot,
    jit: &RunnerJitSnapshot,
    scope: &ConnectionJournal,
    set_id: i64,
    current_set_id: Option<i64>,
    assignment_found: bool,
    expected_name: Option<&str>,
    runner_found: bool,
) -> bool {
    intent.row_id == Some(reservation.intent_id)
        && intent.kind.as_deref() == Some("launch")
        && intent.key.as_deref() == Some(reservation.key.as_str())
        && intent.set_id == Some(set_id)
        && intent.state.is_some()
        && intent.cleanup_proven == Some(false)
        && reservation.binding_digest == scope.binding_digest()
        && current_set_id == Some(set_id)
        && assignment_found
        && intent.acquire_attempted == Some(false)
        && intent.acquire_resolved == Some(false)
        && intent.acquired == Some(false)
        && jit.state_known
        && jit_is_consistent(
            reservation,
            intent,
            jit,
            set_id,
            expected_name,
            runner_found,
        )
        && phase_matches_jit(jit.state, intent.phase)
        && request_ids_are_consistent(intent, jit)
}

fn request_ids_are_consistent(intent: &IntentSnapshot, jit: &RunnerJitSnapshot) -> bool {
    intent.request_id.is_none_or(|request_id| request_id > 0)
        && jit
            .started_request_id
            .is_none_or(|request_id| request_id > 0)
        && jit
            .completed_request_id
            .is_none_or(|request_id| request_id > 0)
        && !(jit.started_request_id.is_some()
            && jit.completed_request_id.is_some()
            && jit.started_request_id != jit.completed_request_id)
        && intent.request_id.is_none_or(|request_id| {
            jit.started_request_id == Some(request_id)
                || jit.completed_request_id == Some(request_id)
        })
}

fn resume_is_eligible(
    reservation: &ReservationRow,
    intent: &IntentSnapshot,
    ownership: &PhysicalRunnerOwnership,
) -> bool {
    matches!(ownership, PhysicalRunnerOwnership::Known(_))
        && reservation.state == ReservationState::Reserved
        && intent.state == Some(IntentState::Pending)
        && !intent.acquire_attempted.unwrap_or(true)
        && !intent.acquire_resolved.unwrap_or(true)
        && !intent.acquired.unwrap_or(true)
        && !intent.jit_requested.unwrap_or(true)
        && intent.runner_container_id.is_none()
        && intent.dind_container_id.is_none()
        && intent.worker_volume.is_none()
        && intent.phase.is_none()
}

fn build_reservation(
    reservation: ReservationRow,
    intent: IntentSnapshot,
    jit: RunnerJitSnapshot,
    runner: Option<RunnerReference>,
    ownership: PhysicalRunnerOwnership,
    resume_eligible: bool,
) -> PhysicalRunnerReservation {
    PhysicalRunnerReservation {
        reservation_id: reservation.reservation_id,
        intent_id: reservation.intent_id,
        ownership,
        reservation: ReservationSnapshot {
            reservation_id: reservation.reservation_id,
            state: reservation.state,
            initial_sample_generation_id: reservation.generation,
            resume_eligible,
            cpu_millicores: reservation.cpu,
            memory_bytes: reservation.memory,
        },
        intent_state: intent.state,
        launch_phase: intent.phase,
        jit_state: jit.state,
        requested_runner_name: jit.requested_name,
        runner,
        witness_request_id: jit.witness_request_id,
        started_request_id: jit.started_request_id,
        completed_request_id: jit.completed_request_id,
        runner_request_id: intent.request_id,
        worker_volume: intent.worker_volume,
        runner_container_id: intent.runner_container_id,
        dind_container_id: intent.dind_container_id,
        intent_cleanup_proven: intent.cleanup_proven.unwrap_or(true),
        worker_cleanup_proven: intent.worker_cleanup_proven.unwrap_or(false),
        remote_cleanup_proven: reservation.remote_cleanup_proven,
        local_cleanup_proven: reservation.local_cleanup_proven,
        completion_cleanup_pending: jit.completion_cleanup_pending,
    }
}

fn read_reservation(row: &turso::Row) -> Result<ReservationRow, HostError> {
    Ok(ReservationRow {
        reservation_id: row.get(0).map_err(|_| HostError::Journal)?,
        intent_id: row.get(1).map_err(|_| HostError::Journal)?,
        key: row.get(2).map_err(|_| HostError::Journal)?,
        state: parse_state(&row.get::<String>(3).map_err(|_| HostError::Journal)?)?,
        binding_digest: row.get(4).map_err(|_| HostError::Journal)?,
        generation: row.get(5).map_err(|_| HostError::Journal)?,
        cpu: nonnegative_u64(row.get(6).map_err(|_| HostError::Journal)?)?,
        memory: nonnegative_u64(row.get(7).map_err(|_| HostError::Journal)?)?,
        remote_cleanup_proven: row.get(8).map_err(|_| HostError::Journal)?,
        local_cleanup_proven: row.get(9).map_err(|_| HostError::Journal)?,
    })
}

fn read_intent(row: &turso::Row) -> Result<IntentSnapshot, HostError> {
    let phase_text: Option<String> = row.get(27).map_err(|_| HostError::Journal)?;
    Ok(IntentSnapshot {
        row_id: row.get(10).map_err(|_| HostError::Journal)?,
        kind: row.get(11).map_err(|_| HostError::Journal)?,
        key: row.get(12).map_err(|_| HostError::Journal)?,
        state: row
            .get::<Option<String>>(13)
            .map_err(|_| HostError::Journal)?
            .as_deref()
            .and_then(|value| IntentState::parse(value).ok()),
        cleanup_proven: row.get(14).map_err(|_| HostError::Journal)?,
        worker_cleanup_proven: row.get(15).map_err(|_| HostError::Journal)?,
        set_id: row.get(16).map_err(|_| HostError::Journal)?,
        runner_id: row.get(17).map_err(|_| HostError::Journal)?,
        runner_name: row.get(18).map_err(|_| HostError::Journal)?,
        request_id: row.get(19).map_err(|_| HostError::Journal)?,
        acquire_attempted: row.get(20).map_err(|_| HostError::Journal)?,
        acquire_resolved: row.get(21).map_err(|_| HostError::Journal)?,
        acquired: row.get(22).map_err(|_| HostError::Journal)?,
        jit_requested: row.get(23).map_err(|_| HostError::Journal)?,
        runner_container_id: row.get(24).map_err(|_| HostError::Journal)?,
        dind_container_id: row.get(25).map_err(|_| HostError::Journal)?,
        worker_volume: row.get(26).map_err(|_| HostError::Journal)?,
        phase: phase_text
            .as_deref()
            .and_then(|value| LaunchPhase::parse(value).ok()),
    })
}

fn read_runner_jit(row: &turso::Row) -> Result<RunnerJitSnapshot, HostError> {
    let state_text: Option<String> = row.get(32).map_err(|_| HostError::Journal)?;
    let (state, state_known) = parse_jit_state(state_text.as_deref());
    Ok(RunnerJitSnapshot {
        intent_id: row.get(28).map_err(|_| HostError::Journal)?,
        set_id: row.get(29).map_err(|_| HostError::Journal)?,
        witness_request_id: row.get(30).map_err(|_| HostError::Journal)?,
        requested_name: row.get(31).map_err(|_| HostError::Journal)?,
        state,
        state_known,
        runner_id: row.get(33).map_err(|_| HostError::Journal)?,
        runner_name: row.get(34).map_err(|_| HostError::Journal)?,
        started_request_id: row.get(35).map_err(|_| HostError::Journal)?,
        completed_request_id: row.get(36).map_err(|_| HostError::Journal)?,
        completion_cleanup_pending: row.get(37).map_err(|_| HostError::Journal)?,
    })
}

fn parse_runner(
    intent: &IntentSnapshot,
    jit: &RunnerJitSnapshot,
    set_id: i64,
) -> Option<RunnerReference> {
    let id = jit.runner_id.filter(|id| *id > 0)?;
    let name = jit.runner_name.clone()?;
    (intent
        .runner_id
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok())
        == Some(id)
        && intent.runner_name.as_deref() == Some(name.as_str())
        && jit.set_id == Some(set_id))
    .then_some(RunnerReference {
        id,
        name,
        runner_scale_set_id: set_id,
    })
}

fn jit_is_consistent(
    reservation: &ReservationRow,
    intent: &IntentSnapshot,
    jit: &RunnerJitSnapshot,
    set_id: i64,
    expected_name: Option<&str>,
    has_runner: bool,
) -> bool {
    match (jit.state, jit.intent_id) {
        (PhysicalRunnerJitState::Absent, None) => {
            intent.jit_requested == Some(false)
                && intent.runner_id.is_none()
                && intent.runner_name.is_none()
                && intent.phase.is_none()
        }
        (PhysicalRunnerJitState::Claimed | PhysicalRunnerJitState::Uncertain, Some(id)) => {
            claimed_jit_is_consistent(
                intent,
                jit,
                id,
                reservation.intent_id,
                set_id,
                expected_name,
            )
        }
        (_, Some(id)) if id == reservation.intent_id => {
            jit.set_id == Some(set_id)
                && jit.witness_request_id.is_some_and(|value| value > 0)
                && jit.requested_name.as_deref() == expected_name
                && has_runner
                && intent.jit_requested == Some(true)
        }
        _ => false,
    }
}

fn claimed_jit_is_consistent(
    intent: &IntentSnapshot,
    jit: &RunnerJitSnapshot,
    jit_id: i64,
    intent_id: i64,
    set_id: i64,
    expected_name: Option<&str>,
) -> bool {
    jit_id == intent_id
        && jit.set_id == Some(set_id)
        && jit.witness_request_id.is_some_and(|value| value > 0)
        && jit.requested_name.as_deref() == expected_name
        && jit.runner_id.is_none()
        && jit.runner_name.is_none()
        && intent.runner_id.is_none()
        && intent.runner_name.is_none()
        && intent.jit_requested == Some(true)
}

pub(super) async fn current_set_id(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    row.get(0).map_err(|_| HostError::Journal)
}

fn parse_assignment(
    scope: &ConnectionJournal,
    set_id: i64,
    key: &str,
) -> Option<AssignmentIdentity> {
    let prefix = format!(
        "v1|{}|{set_id}|remote|6:runner|32:",
        scope.connection_id().as_str()
    );
    let nonce = key.strip_prefix(&prefix)?;
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    AssignmentIdentity::new(
        scope.connection_id(),
        set_id,
        AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_owned(),
        },
    )
    .ok()
}

fn physical_runner_name(identity: &AssignmentIdentity) -> String {
    match identity.key() {
        AssignmentKey::Remote { subject, .. } => format!("v{subject}"),
        _ => String::new(),
    }
}

fn assignment_set_id(scope: &ConnectionJournal, key: &str) -> Option<i64> {
    let prefix = format!("v1|{}|", scope.connection_id().as_str());
    let remainder = key.strip_prefix(&prefix)?;
    let (encoded, tail) = remainder.split_once('|')?;
    if encoded.is_empty()
        || encoded.starts_with('0')
        || !encoded.bytes().all(|byte| byte.is_ascii_digit())
        || !tail.starts_with("remote|6:runner|32:")
    {
        return None;
    }
    let set_id = encoded.parse::<i64>().ok()?;
    (set_id > 0 && set_id.to_string() == encoded).then_some(set_id)
}

fn phase_matches_jit(state: PhysicalRunnerJitState, phase: Option<LaunchPhase>) -> bool {
    match state {
        PhysicalRunnerJitState::Absent => phase.is_none(),
        PhysicalRunnerJitState::Claimed => phase == Some(LaunchPhase::JitRequested),
        PhysicalRunnerJitState::Uncertain => phase == Some(LaunchPhase::JitUncertain),
        PhysicalRunnerJitState::Bound => phase == Some(LaunchPhase::JitBound),
        PhysicalRunnerJitState::StartRequested => phase == Some(LaunchPhase::DockerRequested),
        PhysicalRunnerJitState::StartUncertain => phase == Some(LaunchPhase::DockerUncertain),
        PhysicalRunnerJitState::Ready => phase == Some(LaunchPhase::WorkerReady),
        PhysicalRunnerJitState::Started => phase == Some(LaunchPhase::RunnerStarted),
        PhysicalRunnerJitState::Completed => phase == Some(LaunchPhase::RunnerCompleted),
        PhysicalRunnerJitState::Cleaned => {
            matches!(
                phase,
                Some(LaunchPhase::RunnerCompleted | LaunchPhase::Complete)
            )
        }
        PhysicalRunnerJitState::Unknown => false,
    }
}

fn parse_jit_state(value: Option<&str>) -> (PhysicalRunnerJitState, bool) {
    match value {
        None => (PhysicalRunnerJitState::Absent, true),
        Some("claimed") => (PhysicalRunnerJitState::Claimed, true),
        Some("uncertain") => (PhysicalRunnerJitState::Uncertain, true),
        Some("bound") => (PhysicalRunnerJitState::Bound, true),
        Some("start_requested") => (PhysicalRunnerJitState::StartRequested, true),
        Some("start_uncertain") => (PhysicalRunnerJitState::StartUncertain, true),
        Some("ready") => (PhysicalRunnerJitState::Ready, true),
        Some("started") => (PhysicalRunnerJitState::Started, true),
        Some("completed") => (PhysicalRunnerJitState::Completed, true),
        Some("cleaned") => (PhysicalRunnerJitState::Cleaned, true),
        Some(_) => (PhysicalRunnerJitState::Unknown, false),
    }
}

fn nonnegative_u64(value: i64) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
}
