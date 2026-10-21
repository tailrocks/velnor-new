use crate::{error::HostError, journal::connection::storage::read_transaction};

pub(in crate::journal::connection::reservation::post_jit_retirement::producer_tests) async fn assert_network_phase(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    expected_remove_state: &str,
    expected_phase_count: i64,
) -> Result<(), HostError> {
    read_transaction(&fixture.journal, async |connection| {
        let mut rows = connection
            .query(
                "SELECT network.remove_state,reservation.state,(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress phase WHERE phase.reservation_id=reservation.id AND phase.phase_index=4),(SELECT COUNT(*) FROM physical_attempt_current current WHERE current.reservation_id=reservation.id),state.jit_state,jit.state FROM capacity_reservation_networks network JOIN capacity_reservations reservation ON reservation.id=network.reservation_id JOIN capacity_reservation_provisioning_state state ON state.reservation_id=reservation.id JOIN connection_runner_jits jit ON jit.intent_id=reservation.intent_id AND jit.connection_id=reservation.connection_id WHERE reservation.id=?1",
                [fixture.reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let remove_state: String = row.get(0).map_err(|_| HostError::Journal)?;
        let reservation_state: String = row.get(1).map_err(|_| HostError::Journal)?;
        let phase_count: i64 = row.get(2).map_err(|_| HostError::Journal)?;
        let current_count: i64 = row.get(3).map_err(|_| HostError::Journal)?;
        let jit_state: String = row.get(4).map_err(|_| HostError::Journal)?;
        let runner_jit_state: String = row.get(5).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || remove_state != expected_remove_state
            || reservation_state != "recovering"
            || phase_count != expected_phase_count
            || current_count != 1
            || (expected_phase_count == 1
                && (jit_state != "cleaned" || runner_jit_state != "cleaned"))
            || (expected_phase_count == 0
                && (jit_state == "cleaned" || runner_jit_state == "cleaned"))
        {
            return Err(HostError::Identity);
        }
        Ok(())
    })
    .await
}
