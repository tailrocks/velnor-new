//! Reservation row and runner state decoding.

use super::{
    HostError, IntentSnapshot, IntentState, LaunchPhase, PhysicalRunnerJitState, ReservationRow,
    RunnerJitSnapshot, RunnerReference, nonnegative_u64, parse_jit_state, parse_state,
};

pub(super) fn read_reservation(row: &turso::Row) -> Result<ReservationRow, HostError> {
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

pub(super) fn read_intent(row: &turso::Row) -> Result<IntentSnapshot, HostError> {
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

pub(super) fn read_runner_jit(row: &turso::Row) -> Result<RunnerJitSnapshot, HostError> {
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

pub(super) fn parse_runner(
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

pub(super) fn jit_is_consistent(
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
                && intent.runner_name.as_deref() == expected_name
                && matches!(intent.phase, None | Some(LaunchPhase::ProvisioningHeld))
        }
        (PhysicalRunnerJitState::Absent, Some(id)) => {
            id == reservation.intent_id
                && jit.set_id == Some(set_id)
                && jit.witness_request_id.is_none()
                && jit.requested_name.is_none()
                && jit.runner_id.is_none()
                && jit.runner_name.is_none()
                && intent.jit_requested == Some(false)
                && intent.runner_id.is_none()
                && intent.runner_name.as_deref() == expected_name
                && matches!(intent.phase, None | Some(LaunchPhase::ProvisioningHeld))
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

pub(super) fn claimed_jit_is_consistent(
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
