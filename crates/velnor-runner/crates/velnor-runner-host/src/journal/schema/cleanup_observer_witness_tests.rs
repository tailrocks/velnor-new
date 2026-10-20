//! Database guarantees for the persisted cleanup observer witness.

use crate::error::HostError;
use crate::journal::request_lifecycle::tests::NativePreChallengeFixture;

#[tokio::test]
async fn persisted_witness_rejects_digest_and_context_tampering() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_pre_challenge_fixture().await?;
    let NativePreChallengeFixture {
        _journal: journal, ..
    } = &fixture;
    let reservation_id = fixture.initialized.reservation_id;
    let before = fixture
        .scope
        .storage_cleanup_witness(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let connection = journal.connection().await?;

    let digest_tamper = "0".repeat(64);
    if digest_tamper == before.binding_sha256() {
        return Err(HostError::Journal);
    }
    if connection
        .execute(
            "UPDATE capacity_reservation_cleanup_observer_witnesses SET binding_sha256 = ?1 WHERE reservation_id = ?2",
            (digest_tamper.as_str(), reservation_id),
        )
        .await
        .is_ok()
    {
        return Err(HostError::Journal);
    }

    if connection
        .execute(
            "UPDATE capacity_reservation_cleanup_observer_witnesses SET engine_id = ?1 WHERE reservation_id = ?2",
            ("tampered-engine", reservation_id),
        )
        .await
        .is_ok()
    {
        return Err(HostError::Journal);
    }

    if connection
        .execute(
            "DELETE FROM capacity_reservation_cleanup_observer_witnesses WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .is_ok()
    {
        return Err(HostError::Journal);
    }

    let after = fixture
        .scope
        .storage_cleanup_witness(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if before != after {
        return Err(HostError::Journal);
    }
    Ok(())
}
