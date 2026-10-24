use super::super::super::helpers::{
    record_observed_storage_cleanup, record_owned_network_cleanup, settle_storage_metadata,
};
use super::super::replay_link;
use super::LinkedLifecycle;
use crate::error::HostError;
use crate::journal::{ConnectionJournal, MetadataSettlementResult};

pub(in crate::journal::request_lifecycle::tests::linked_request_reopen) async fn check_cleanup_and_settlement(
    fixture: &LinkedLifecycle,
) -> Result<(), HostError> {
    let scope = &fixture.scope;
    replay_link(
        scope,
        fixture.witness_request_id,
        fixture.nonce,
        'f',
        fixture.reservation_id,
        fixture.intent_id,
    )
    .await?;
    let actual = scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest {
            request_id: fixture.actual_request_id,
        })
        .await?;
    assert_eq!(
        scope
            .record_job_completed(
                &actual,
                902,
                &fixture.source,
                Some("success"),
                Some(&fixture.runner),
                Some(&fixture.job_id),
            )
            .await?,
        Some(fixture.intent_id)
    );
    assert_eq!(
        scope
            .reservation(&fixture.physical)
            .await?
            .map(|reservation| reservation.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    record_observed_storage_cleanup(scope, fixture.reservation_id).await?;
    scope.record_remote_cleanup(fixture.reservation_id).await?;
    scope.record_local_cleanup(fixture.reservation_id).await?;
    assert!(matches!(
        settle_storage_metadata(scope, &fixture.provisioned, '7').await,
        Err(HostError::Journal)
    ));
    assert_failed_settlement_stays_reserved(fixture).await?;

    record_owned_network_cleanup(scope, fixture.reservation_id).await?;
    install_release_rejection(scope, fixture.reservation_id).await?;
    assert!(matches!(
        settle_storage_metadata(scope, &fixture.provisioned, '9').await,
        Err(HostError::Journal)
    ));
    assert_failed_settlement_stays_reserved(fixture).await?;
    remove_release_rejection(scope).await?;
    crate::journal::connection::storage::read_transaction(scope.journal_ref(), async |connection| {
        let changed = connection
            .execute(
                "UPDATE capacity_reservations SET state = 'uncertain' WHERE id = ?1 AND state = 'reserved'",
                [fixture.reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        Ok(())
    })
    .await?;
    assert_eq!(
        settle_storage_metadata(scope, &fixture.provisioned, 'a').await?,
        MetadataSettlementResult::Settled
    );
    assert_eq!(
        scope
            .reservation(&fixture.physical)
            .await?
            .map(|reservation| reservation.state),
        Some(crate::journal::ReservationState::Released)
    );
    assert!(scope.intent(fixture.intent_id).await?.cleanup_proven);
    Ok(())
}

async fn assert_failed_settlement_stays_reserved(
    fixture: &LinkedLifecycle,
) -> Result<(), HostError> {
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture.physical)
            .await?
            .map(|reservation| reservation.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    assert!(
        !fixture
            .scope
            .intent(fixture.intent_id)
            .await?
            .cleanup_proven
    );
    Ok(())
}

async fn install_release_rejection(
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<(), HostError> {
    crate::journal::connection::storage::read_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute(
                &format!(
                    "CREATE TRIGGER reject_test_reservation_release BEFORE UPDATE OF state ON capacity_reservations WHEN NEW.id = {reservation_id} AND NEW.state = 'released' BEGIN SELECT RAISE(ABORT, 'test release rejection'); END"
                ),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await
}

async fn remove_release_rejection(scope: &ConnectionJournal) -> Result<(), HostError> {
    crate::journal::connection::storage::read_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute("DROP TRIGGER reject_test_reservation_release", ())
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await
}
