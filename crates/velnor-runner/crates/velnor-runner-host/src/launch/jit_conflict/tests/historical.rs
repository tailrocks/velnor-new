// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    JitOutcome, TestResult, binding,
    case::{Case, Schema},
    scalar, seed,
};
use crate::journal::Journal;

#[tokio::test(flavor = "multi_thread")]
async fn historical_409_local_progress_and_its_live_claim_survive_upgrade_and_resume()
-> TestResult<()> {
    Box::pin(preserves(false)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn historical_409_settlement_remains_byte_identical_and_excluded_after_upgrade()
-> TestResult<()> {
    Box::pin(preserves(true)).await
}

async fn preserves(settled: bool) -> TestResult<()> {
    let mut case = Case::new(Schema::Historical45, JitOutcome::Conflict, false).await?;
    let candidates = case.scope.recoverable_jit_conflict_candidates().await?;
    let claim = case
        .scope
        .claim_jit_conflict_candidate(candidates.first().ok_or("409 candidate")?, 240)
        .await?
        .ok_or("409 claim")?;
    let stored=scalar(&case.journal,"SELECT snapshot_sha256 FROM physical_attempt_jit_conflict_recovery_claims WHERE reservation_id=2").await?;
    assert_eq!(stored, historical_snapshot(&case.journal).await?);
    super::super::local::ensure_local_absence(
        &case.runtime()?,
        &case.docker,
        &case.scope,
        &case.engine,
        &claim,
    )
    .await?;
    if settled {
        case.finish(&claim, true).await?;
    }
    let rows = proof_rows(&case.journal).await?;
    seed::execute(
        &case.journal,
        "DROP VIEW physical_attempt_jit_recovery_sources",
    )
    .await?;
    assert_eq!(version(&case.journal).await?, 45);
    assert_eq!(scalar(&case.journal,"SELECT CAST(count(*) AS TEXT) FROM sqlite_schema WHERE name IN ('physical_attempt_jit_recovery_sources','physical_attempt_jit_interrupted_claims','jit_interrupted_claim_insert_guard')").await?,"0");
    case = case.reopen(Schema::Historical45).await?;
    assert_eq!(proof_rows(&case.journal).await?, rows);
    case = case.reopen(Schema::Current).await?;
    assert_eq!(proof_rows(&case.journal).await?, rows);
    case = case.reopen(Schema::Current).await?;
    assert_eq!(proof_rows(&case.journal).await?, rows);
    if settled {
        assert!(
            case.scope
                .recoverable_jit_conflict_candidates()
                .await?
                .is_empty()
        );
    } else {
        case.scope.validate_jit_conflict_claim(&claim).await?;
        case.finish(&claim, true).await?;
    }
    assert_eq!(scalar(&case.journal,"SELECT CAST(count(*) AS TEXT) FROM physical_attempt_jit_conflict_settlements WHERE reservation_id=2").await?,"1");
    assert_eq!(
        scalar(
            &case.journal,
            "SELECT state FROM capacity_reservations WHERE id=2"
        )
        .await?,
        "released"
    );
    Ok(())
}

async fn proof_rows(journal: &Journal) -> TestResult<Vec<String>> {
    let connection = journal.connection().await?;
    let mut result = Vec::new();
    for table in [
        "physical_attempt_jit_conflict_receipts",
        "physical_attempt_jit_conflict_recovery_claims",
        "physical_attempt_jit_conflict_recovery_progress",
        "physical_attempt_jit_conflict_settlements",
    ] {
        let mut rows = connection
            .query(&format!("SELECT * FROM {table} ORDER BY rowid"), ())
            .await?;
        while let Some(row) = rows.next().await? {
            let values = (0..row.column_count())
                .map(|index| row.get_value(index).map(|value| format!("{value:?}")))
                .collect::<Result<Vec<_>, _>>()?;
            result.push(format!("{table}:{}", values.join(",")));
        }
    }
    Ok(result)
}

async fn historical_snapshot(journal: &Journal) -> TestResult<String> {
    let connection = journal.connection().await?;
    let mut rows=connection.query("SELECT claim.lineage_id,claim.generation,claim.reservation_id,claim.intent_id,claim.connection_id,claim.scale_set_id,claim.witness_request_id,claim.physical_nonce,claim.binding_digest,claim.source_digest,claim.acquire_response_digest,claim.authority_generation,claim.authority_digest,claim.engine_id,claim.docker_root_digest,state.instance_binding_digest,claim.requested_name FROM physical_attempt_jit_conflict_recovery_claims claim JOIN capacity_reservation_provisioning_state state ON state.reservation_id=claim.reservation_id WHERE claim.reservation_id=2",()).await?;
    let row = rows.next().await?.ok_or("historical claim")?;
    let mut identity = vec![serde_json::json!(
        "velnor-jit-conflict-recovery-identity-v1"
    )];
    for index in 0..17 {
        identity.push(if [0, 1, 2, 3, 5, 6, 11].contains(&index) {
            serde_json::json!(row.get::<i64>(index)?)
        } else {
            serde_json::json!(row.get::<String>(index)?)
        });
    }
    Ok(binding::digest(&serde_json::to_vec(&identity)?))
}

async fn version(journal: &Journal) -> TestResult<i64> {
    let connection = journal.connection().await?;
    let mut rows = connection.query("PRAGMA user_version", ()).await?;
    Ok(rows.next().await?.ok_or("version")?.get(0)?)
}
