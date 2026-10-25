use crate::error::HostError;
use crate::journal::request_lifecycle::RunnerJitClaim;
use crate::journal::{CapacityLimits, Journal};

const SET_ID: i64 = 501;

impl crate::journal::ConnectionJournal {
    pub(crate) async fn test_jit_conflict_receipt_count(
        &self,
        intent_id: i64,
    ) -> Result<i64, HostError> {
        crate::journal::connection::storage::read_transaction(self.journal_ref(), async |connection| {
            let mut rows = connection.query("SELECT COUNT(*) FROM physical_attempt_jit_conflict_receipts WHERE intent_id=?1", [intent_id]).await.map_err(|_| HostError::Journal)?;
            rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?.get(0).map_err(|_| HostError::Journal)
        }).await
    }
}

#[tokio::test]
async fn exact_jit_conflict_receipt_is_idempotent_and_rejects_another_claim()
-> Result<(), HostError> {
    use super::helpers::{
        Scratch, binding, created_scope, provision_acquired_request_with_initialization,
    };

    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let repository = binding("jit-conflict-receipt", 101)?;
    let scope = created_scope(&journal, &repository, SET_ID).await?;
    let physical = provision_acquired_request_with_initialization(&scope, 70, 'b', 'b').await?;
    let runner_name = format!("v{}", "b".repeat(32));
    let (policy_generation, policy_digest) = scope.test_active_authority_identity().await?;
    let claim = scope
        .claim_runner_jit_for_request(
            physical.intent_id,
            physical.reservation_id,
            70,
            &runner_name,
            policy_generation,
            &policy_digest,
        )
        .await?
        .ok_or(HostError::Journal)?;

    assert_rejected_receipt_claims_remain_charged(
        &scope,
        physical.intent_id,
        physical.reservation_id,
        70,
        &runner_name,
        policy_generation,
        &policy_digest,
    )
    .await?;
    scope
        .mark_runner_jit_conflict_received(
            claim,
            physical.reservation_id,
            policy_generation,
            &policy_digest,
        )
        .await?;
    assert_receipt_is_idempotent_and_rejects_other_claims(
        &scope,
        physical.intent_id,
        physical.reservation_id,
        &runner_name,
        policy_generation,
        &policy_digest,
    )
    .await?;
    Ok(())
}

async fn assert_receipt_is_idempotent_and_rejects_other_claims(
    scope: &crate::journal::ConnectionJournal,
    intent_id: i64,
    reservation_id: i64,
    requested_name: &str,
    authority_generation: u64,
    authority_digest: &str,
) -> Result<(), HostError> {
    scope
        .mark_runner_jit_conflict_received(
            test_claim(scope, intent_id, 70, requested_name),
            reservation_id,
            authority_generation,
            authority_digest,
        )
        .await?;
    assert_eq!(scope.test_jit_conflict_receipt_count(intent_id).await?, 1);
    let mismatched = scope
        .mark_runner_jit_conflict_received(
            test_claim(scope, intent_id, 71, requested_name),
            reservation_id,
            authority_generation,
            authority_digest,
        )
        .await;
    assert!(mismatched.is_err());
    let forged_terminal = crate::journal::connection::storage::in_transaction(
        scope.journal_ref(),
        async |connection| {
            Ok(connection
                .execute(
                    "INSERT INTO physical_attempt_outcomes (lineage_id, generation, reservation_id, outcome, proof_digest, settlement_digest, created_at) SELECT lineage_id, generation, reservation_id, 'jit_conflict_retired', lower(hex(randomblob(32))), lower(hex(randomblob(32))), unixepoch() FROM physical_attempts WHERE intent_id=?1",
                    [intent_id],
                )
                .await
                .is_err())
        },
    )
    .await?;
    assert!(forged_terminal);
    assert_eq!(scope.test_jit_conflict_receipt_count(intent_id).await?, 1);
    Ok(())
}

async fn assert_rejected_receipt_claims_remain_charged(
    scope: &crate::journal::ConnectionJournal,
    intent_id: i64,
    reservation_id: i64,
    witness_request_id: i64,
    requested_name: &str,
    authority_generation: u64,
    authority_digest: &str,
) -> Result<(), HostError> {
    let mut wrong_digest = authority_digest.to_owned();
    let replacement = if wrong_digest.starts_with('0') {
        '1'
    } else {
        '0'
    };
    wrong_digest.replace_range(0..1, &replacement.to_string());
    for (reservation, generation, digest) in [
        (
            reservation_id + 1,
            authority_generation,
            authority_digest.to_owned(),
        ),
        (
            reservation_id,
            authority_generation + 1,
            authority_digest.to_owned(),
        ),
        (reservation_id, authority_generation, wrong_digest),
    ] {
        let result = scope
            .mark_runner_jit_conflict_received(
                test_claim(scope, intent_id, witness_request_id, requested_name),
                reservation,
                generation,
                &digest,
            )
            .await;
        assert!(result.is_err());
        assert_eq!(scope.test_jit_conflict_receipt_count(intent_id).await?, 0);
    }
    let rows = scope.physical_runner_reservations().await?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.reservation.state,
        crate::journal::ReservationState::Reserved
    );
    assert_eq!(format!("{:?}", row.jit_state), "Claimed");
    Ok(())
}

fn test_claim(
    scope: &crate::journal::ConnectionJournal,
    intent_id: i64,
    witness_request_id: i64,
    requested_name: &str,
) -> RunnerJitClaim {
    RunnerJitClaim {
        connection_id: scope.connection_id().clone(),
        scale_set_id: SET_ID,
        intent_id,
        witness_request_id,
        requested_name: requested_name.to_owned(),
    }
}
