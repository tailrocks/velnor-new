use super::{IntentSnapshot, ReservationRow, RunnerJitSnapshot};
use super::{jit_is_consistent, phase_matches_jit, request_ids_are_consistent};
use crate::journal::connection::ConnectionJournal;

#[derive(Clone, Copy)]
pub(super) struct ReservationIdentityContext<'a> {
    pub(super) reservation: &'a ReservationRow,
    pub(super) intent: &'a IntentSnapshot,
    pub(super) jit: &'a RunnerJitSnapshot,
    pub(super) scope: &'a ConnectionJournal,
    pub(super) set_id: i64,
    pub(super) current_set_id: Option<i64>,
    pub(super) assignment_found: bool,
    pub(super) expected_name: Option<&'a str>,
    pub(super) runner_found: bool,
    pub(super) request_link_is_consistent: bool,
}

pub(super) fn reservation_identity_is_known(context: &ReservationIdentityContext<'_>) -> bool {
    let ReservationIdentityContext {
        reservation,
        intent,
        jit,
        scope,
        set_id,
        current_set_id,
        assignment_found,
        expected_name,
        runner_found,
        request_link_is_consistent,
    } = *context;
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
        && request_link_is_consistent
}
