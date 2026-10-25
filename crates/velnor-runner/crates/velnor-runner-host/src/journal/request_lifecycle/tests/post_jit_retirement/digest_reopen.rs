use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::{Journal, MetadataSettlementResult};

use super::{
    helpers, prepared_fixture, record_storage_and_network_proofs, seed_mocked_external_evidence,
};

#[tokio::test]
async fn reopen_rejects_changed_capacity_settlement_digest() -> Result<(), HostError> {
    Box::pin(reopen_rejects_changed_capacity_settlement_digest_inner()).await
}

async fn reopen_rejects_changed_capacity_settlement_digest_inner() -> Result<(), HostError> {
    let fixture = Box::pin(settled_fixture('c')).await?;
    Box::pin(tamper_phase_five_digest(&fixture)).await?;
    assert!(
        Box::pin(Journal::open(&fixture.database_path))
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn reopen_rejects_changed_terminal_digest() -> Result<(), HostError> {
    Box::pin(reopen_rejects_changed_terminal_digest_inner()).await
}

async fn reopen_rejects_changed_terminal_digest_inner() -> Result<(), HostError> {
    let fixture = Box::pin(settled_fixture('d')).await?;
    Box::pin(tamper_terminal_digest(&fixture)).await?;
    assert!(
        Box::pin(Journal::open(&fixture.database_path))
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn reopen_rejects_changed_pending_claim_phase_evidence() -> Result<(), HostError> {
    let fixture = Box::pin(prepared_fixture()).await?;
    Box::pin(seed_mocked_external_evidence(&fixture)).await?;
    Journal::open(&fixture.database_path).await?;
    Box::pin(tamper_pending_phase_zero(&fixture)).await?;
    assert!(Journal::open(&fixture.database_path).await.is_err());
    Ok(())
}

async fn settled_fixture(nonce: char) -> Result<super::NativeStartedIntakeFixture, HostError> {
    let fixture = Box::pin(prepared_fixture()).await?;
    Box::pin(seed_mocked_external_evidence(&fixture)).await?;
    Box::pin(record_storage_and_network_proofs(&fixture)).await?;
    let claim = Box::pin(helpers::create_cleaned_settlement_probe(
        &fixture.scope,
        &fixture.provisioned,
        nonce,
    ))
    .await?;
    let sample = Box::pin(helpers::settlement_sample(&fixture.scope, &claim)).await?;
    let outcome = Box::pin(
        fixture
            .scope
            .record_storage_metadata_settlement(&claim, sample),
    )
    .await?;
    assert_eq!(outcome, MetadataSettlementResult::Settled);
    Ok(fixture)
}

async fn tamper_phase_five_digest(
    fixture: &super::NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let scope = fixture.scope.clone();
    in_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute(
                "DROP TRIGGER physical_attempt_post_jit_retirement_progress_no_update",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "UPDATE physical_attempt_post_jit_retirement_progress SET evidence_sha256=?1 WHERE reservation_id=?2 AND claim_generation=1 AND phase_index=5",
                ("0".repeat(64), fixture.reservation_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        restore_progress_immutability(connection).await
    })
    .await
}

async fn tamper_terminal_digest(
    fixture: &super::NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let scope = fixture.scope.clone();
    in_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute(
                "DROP TRIGGER physical_attempt_post_jit_retirements_no_update",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "UPDATE physical_attempt_post_jit_retirements SET proof_digest=?1,settlement_digest=?1 WHERE reservation_id=?2",
                ("0".repeat(64), fixture.reservation_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "CREATE TRIGGER physical_attempt_post_jit_retirements_no_update BEFORE UPDATE ON physical_attempt_post_jit_retirements BEGIN SELECT RAISE(ABORT,'Keep the physical retirement record.'); END",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await
}

async fn tamper_pending_phase_zero(
    fixture: &super::NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let scope = fixture.scope.clone();
    in_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute(
                "DROP TRIGGER physical_attempt_post_jit_retirement_progress_no_update",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "UPDATE physical_attempt_post_jit_retirement_progress SET evidence_sha256=?1 WHERE reservation_id=?2 AND claim_generation=1 AND phase_index=0",
                ("0".repeat(64), fixture.reservation_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        restore_progress_immutability(connection).await
    })
    .await
}

async fn restore_progress_immutability(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TRIGGER physical_attempt_post_jit_retirement_progress_no_update BEFORE UPDATE ON physical_attempt_post_jit_retirement_progress BEGIN SELECT RAISE(ABORT,'Keep the physical retirement record.'); END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
