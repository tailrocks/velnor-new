use crate::{
    error::HostError,
    journal::{IntentState, ReservationState},
    launch::bootstrap::recover_pre_jit_reservation,
};

use super::super::state::PreparedConnection;
use super::PositivePollContext;

pub(super) async fn reconcile(
    prepared: &PreparedConnection,
    context: &PositivePollContext<'_>,
) -> Result<(), HostError> {
    let reservations = prepared.scope.physical_runner_reservations().await?;
    for reservation in reservations {
        if reservation.reservation.state != ReservationState::Uncertain
            || reservation.intent_state != Some(IntentState::Uncertain)
            || reservation.jit_state != crate::journal::PhysicalRunnerJitState::Absent
            || reservation.acquired_request.is_none()
            || reservation.ownership.assignment().is_none()
        {
            continue;
        }
        if let Err(error) = recover_pre_jit_reservation(
            context.docker,
            context.journal,
            &prepared.scope,
            context.selected_engine,
            context.worker_images,
            reservation.reservation_id,
            context.release_read_secret,
        )
        .await
        {
            eprintln!(
                "The controller keeps an uncertain pre-JIT reservation charged. connection={} reservation={} reason={error}",
                prepared.binding.connection_id, reservation.reservation_id
            );
        }
    }
    Ok(())
}
