use crate::error::HostError;
use crate::journal::MetadataSettlementResult;
use crate::journal::connection::storage::read_transaction;
use crate::journal::request_lifecycle::tests::{helpers, review_counterexamples};
use sha2::{Digest, Sha256};

#[tokio::test]
async fn normal_outcome_insert_is_rejected_after_retirement_claim() -> Result<(), HostError> {
    let fixture = super::prepared_fixture().await?;
    super::seed_mocked_external_evidence(&fixture).await?;
    let attempt = fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let connection = fixture.journal.connection().await?;
    let error = connection
        .execute(
            "INSERT INTO physical_attempt_outcomes (lineage_id,generation,reservation_id,outcome,proof_digest,settlement_digest,created_at) VALUES (?1,?2,?3,'normal_completion',?4,?4,unixepoch())",
            (
                attempt.lineage_id,
                attempt.generation,
                attempt.reservation_id,
                "a".repeat(64),
            ),
        )
        .await
        .expect_err("The retirement claim must prevent a normal completion record.");
    assert!(
        error
            .to_string()
            .contains("The physical attempt has a retirement fence."),
        "The check returned a different error: {error}"
    );
    assert_retirement_claim_remains_only_terminal(&fixture).await
}

#[tokio::test]
async fn retirement_claim_insert_is_rejected_after_normal_outcome() -> Result<(), HostError> {
    let ready = review_counterexamples::ready().await?;
    let reservation = ready
        .scope
        .reservation(&ready.physical)
        .await?
        .ok_or(HostError::Journal)?;
    let cleanup_generation = helpers::record_runner_completion_for_cleanup(
        &ready.scope,
        &ready.request,
        ready.intent,
        Some(701),
        703,
        &helpers::event_source(101, 1070),
        &ready.runner,
    )
    .await?;
    settle_normal_completion(&ready, reservation.reservation_id, cleanup_generation).await?;
    assert_normal_outcome_is_current_terminal(&ready.journal, reservation.reservation_id).await?;

    let attempt = read_historical_attempt(&ready.journal, reservation.reservation_id).await?;
    let (acquire_digest, instance_digest, runner_id, runner_name) = read_claim_state(
        &ready.journal,
        reservation.reservation_id,
        attempt.connection_id.as_str(),
        attempt.witness_request_id,
    )
    .await?;
    let snapshot = identity_snapshot(
        &attempt,
        &acquire_digest,
        &instance_digest,
        runner_id,
        &runner_name,
    );
    let error = insert_claim_after_terminal(
        &ready,
        &attempt,
        &acquire_digest,
        &instance_digest,
        runner_id,
        &runner_name,
        &snapshot,
    )
    .await?;
    assert!(
        error.contains("The physical retirement claim is invalid."),
        "The check returned a different error: {error}"
    );
    assert_normal_outcome_is_current_terminal(&ready.journal, reservation.reservation_id).await?;
    Ok(())
}

async fn settle_normal_completion(
    ready: &review_counterexamples::Ready,
    reservation_id: i64,
    cleanup_generation: i64,
) -> Result<(), HostError> {
    helpers::record_owned_storage_cleanup(&ready.scope, &ready.provisioned).await?;
    ready.scope.record_remote_cleanup(reservation_id).await?;
    ready.scope.record_local_cleanup(reservation_id).await?;
    helpers::record_owned_network_cleanup(&ready.scope, reservation_id).await?;
    helpers::record_runner_absent_for_cleanup(&ready.scope, ready.intent, cleanup_generation)
        .await?;
    let claim =
        helpers::create_cleaned_settlement_probe(&ready.scope, &ready.provisioned, '4').await?;
    let sample = helpers::settlement_sample(&ready.scope, &claim).await?;
    if ready
        .scope
        .record_storage_metadata_settlement(&claim, sample)
        .await?
        != MetadataSettlementResult::Settled
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn assert_retirement_claim_remains_only_terminal(
    fixture: &super::NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let scope = fixture.scope.clone();
    read_transaction(scope.journal_ref(), async |connection| {
        super::assert_row(
            connection,
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1",
            fixture.reservation_id,
            1,
        )
        .await?;
        super::assert_row(
            connection,
            "SELECT COUNT(*) FROM physical_attempt_outcomes WHERE reservation_id=?1",
            fixture.reservation_id,
            0,
        )
        .await
    })
    .await
}

async fn assert_normal_outcome_is_current_terminal(
    journal: &crate::journal::Journal,
    reservation_id: i64,
) -> Result<(), HostError> {
    read_transaction(journal, async |connection| {
        super::assert_row(
            connection,
            "SELECT COUNT(*) FROM physical_attempt_outcomes WHERE reservation_id=?1 AND outcome='normal_completion'",
            reservation_id,
            1,
        )
        .await?;
        super::assert_row(
            connection,
            "SELECT COUNT(*) FROM physical_attempt_current WHERE reservation_id=?1",
            reservation_id,
            0,
        )
        .await?;
        super::assert_row(
            connection,
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1",
            reservation_id,
            0,
        )
        .await
    })
    .await
}

struct HistoricalAttempt {
    lineage_id: i64,
    generation: i64,
    reservation_id: i64,
    intent_id: i64,
    connection_id: String,
    scale_set_id: i64,
    witness_request_id: i64,
    binding_digest: String,
    source_digest: String,
    engine_id: String,
    docker_root_digest: String,
}

async fn read_historical_attempt(
    journal: &crate::journal::Journal,
    reservation_id: i64,
) -> Result<HistoricalAttempt, HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT lineage_id,generation,reservation_id,intent_id,connection_id,scale_set_id,witness_request_id,binding_digest,source_digest,engine_id,docker_root_digest FROM physical_attempts WHERE reservation_id=?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let attempt = HistoricalAttempt {
        lineage_id: row.get(0).map_err(|_| HostError::Journal)?,
        generation: row.get(1).map_err(|_| HostError::Journal)?,
        reservation_id: row.get(2).map_err(|_| HostError::Journal)?,
        intent_id: row.get(3).map_err(|_| HostError::Journal)?,
        connection_id: row.get(4).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(5).map_err(|_| HostError::Journal)?,
        witness_request_id: row.get(6).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(7).map_err(|_| HostError::Journal)?,
        source_digest: row.get(8).map_err(|_| HostError::Journal)?,
        engine_id: row.get(9).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(10).map_err(|_| HostError::Journal)?,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(attempt)
}

async fn read_claim_state(
    journal: &crate::journal::Journal,
    reservation_id: i64,
    connection_id: &str,
    witness_request_id: i64,
) -> Result<(String, String, i64, String), HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT auth.acquire_response_digest,state.instance_binding_digest,state.runner_id,state.runner_name FROM capacity_reservation_provisioning_state state JOIN connection_request_authorizations auth ON auth.connection_id=?2 AND auth.request_id=?3 AND auth.scale_set_id=(SELECT scale_set_id FROM connection_intents WHERE id=(SELECT intent_id FROM capacity_reservations WHERE id=?1)) WHERE state.reservation_id=?1 AND auth.acquire_state='acquired'",
            (reservation_id, connection_id, witness_request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let values = (
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
        row.get(2).map_err(|_| HostError::Journal)?,
        row.get(3).map_err(|_| HostError::Journal)?,
    );
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(values)
}

fn identity_snapshot(
    attempt: &HistoricalAttempt,
    acquire_digest: &str,
    instance_digest: &str,
    runner_id: i64,
    runner_name: &str,
) -> String {
    let value = serde_json::json!([
        "velnor-post-jit-retirement-identity-v1",
        attempt.lineage_id,
        attempt.generation,
        attempt.reservation_id,
        attempt.intent_id,
        attempt.connection_id,
        attempt.scale_set_id,
        attempt.witness_request_id,
        attempt.binding_digest,
        attempt.source_digest,
        acquire_digest,
        attempt.engine_id,
        attempt.docker_root_digest,
        instance_digest,
        runner_id,
        runner_name,
    ]);
    digest(
        serde_json::to_string(&value)
            .expect("The program must convert the JSON value to bytes.")
            .as_bytes(),
    )
}

async fn insert_claim_after_terminal(
    ready: &review_counterexamples::Ready,
    attempt: &HistoricalAttempt,
    acquire_digest: &str,
    instance_digest: &str,
    runner_id: i64,
    runner_name: &str,
    snapshot: &str,
) -> Result<String, HostError> {
    let connection = ready.journal.connection().await?;
    match connection
        .execute(
            "INSERT INTO physical_attempt_post_jit_retirement_claims (reservation_id,claim_generation,lineage_id,generation,intent_id,connection_id,scale_set_id,witness_request_id,binding_digest,source_digest,acquire_response_digest,engine_id,docker_root_digest,instance_binding_digest,runner_id,runner_name,snapshot_sha256,lease_until,created_at) VALUES (?1,1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,unixepoch()+60,unixepoch())",
            (
                attempt.reservation_id,
                attempt.lineage_id,
                attempt.generation,
                attempt.intent_id,
                attempt.connection_id.as_str(),
                attempt.scale_set_id,
                attempt.witness_request_id,
                attempt.binding_digest.as_str(),
                attempt.source_digest.as_str(),
                acquire_digest,
                attempt.engine_id.as_str(),
                attempt.docker_root_digest.as_str(),
                instance_digest,
                runner_id,
                runner_name,
                snapshot,
            ),
        )
        .await
    {
        Ok(_) => Err(HostError::Journal),
        Err(error) => Ok(error.to_string()),
    }
}

fn digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing into String cannot fail");
    }
    output
}
