use super::migration::{ProofFacts, proof_facts, user_version};
use super::*;

pub(super) async fn verify_failed_upgrades(
    connection: &turso::Connection,
    reservation_id: i64,
    before: &ProofFacts,
) -> Result<(), HostError> {
    let expected_version = user_version(connection).await?;
    for failpoint in [
        super::super::upgrade_v28::UpgradeFailpoint::AfterDecisionDrop,
        super::super::upgrade_v28::UpgradeFailpoint::AfterProofCopy,
    ] {
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let failed = super::super::upgrade_v28::upgrade_with_test_failure(connection, failpoint)
            .await
            .is_err();
        connection
            .execute("ROLLBACK", ())
            .await
            .map_err(|_| HostError::Journal)?;
        verify_rollback(connection, reservation_id, before, expected_version, failed).await?;
    }
    Ok(())
}

async fn verify_rollback(
    connection: &turso::Connection,
    reservation_id: i64,
    before: &ProofFacts,
    expected_version: i64,
    failed: bool,
) -> Result<(), HostError> {
    let version = user_version(connection).await?;
    if !failed
        || version != expected_version
        || &proof_facts(connection, reservation_id).await? != before
    {
        return Err(HostError::Journal);
    }
    super::super::guards::bootstrap_decision_transition_guards(connection, true).await?;
    super::super::guards::bootstrap_proof_transition_guards(connection, true).await?;
    super::super::validate_v27_objects(connection).await
}
