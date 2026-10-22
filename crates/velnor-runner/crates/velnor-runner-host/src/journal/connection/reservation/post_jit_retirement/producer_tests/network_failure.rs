use crate::{error::HostError, launch::bootstrap::cleanup_runtime_fixture};

#[test]
fn uncertain_marker_failure_keeps_network_reservation_charged() -> Result<(), HostError> {
    super::retry::run_on_test_stack(|| async {
        let fixture = cleanup_runtime_fixture().await?;
        let selected = super::selected_engine(&fixture.scope, fixture.reservation_id).await?;
        let claim = super::prepare_local_remote(&fixture).await?;
        fixture
            .scope
            .record_retirement_storage_phase(&claim)
            .await?;

        let connection = fixture.journal.connection().await?;
        connection
            .execute(
                "CREATE TRIGGER reject_network_phase_failure BEFORE INSERT ON physical_attempt_post_jit_retirement_progress WHEN NEW.phase_index = 4 BEGIN SELECT RAISE(ABORT, 'test rollback'); END",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "CREATE TRIGGER reject_uncertain_network_marker BEFORE UPDATE OF remove_state ON capacity_reservation_networks WHEN OLD.remove_state = 'requested' AND NEW.remove_state = 'uncertain' BEGIN SELECT RAISE(ABORT, 'test marker failure'); END",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;

        let result = super::remove_network_for_retirement(&fixture, &claim, &selected).await;

        connection
            .execute("DROP TRIGGER reject_network_phase_failure", ())
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute("DROP TRIGGER reject_uncertain_network_marker", ())
            .await
            .map_err(|_| HostError::Journal)?;
        assert_eq!(result, Err(HostError::Journal));
        super::network_id::assert_network_phase(&fixture, "requested", 0).await
    })
}
