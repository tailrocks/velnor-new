// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    JitOutcome, TestResult,
    case::{Case, Schema},
    github::Github,
    scalar,
};
use crate::journal::{JitConflictRecoveryClaim, Journal};

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_remote_absence_is_refused_by_the_journal_and_its_sql_boundary()
-> TestResult<()> {
    Box::pin(refuses_unfenced_absence()).await
}

async fn refuses_unfenced_absence() -> TestResult<()> {
    let (case, claim) = Box::pin(prepared(Schema::Current, JitOutcome::Interrupted)).await?;
    let receipt = empty_lookup(&claim)?;
    let connection = case.journal.connection().await?;
    assert!(connection.execute(
        "INSERT INTO physical_attempt_jit_conflict_recovery_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,runner_name,runner_scale_set_id,request_sha256,response_sha256,lookup_status,lookup_count,observed_at_unix_ms) SELECT reservation_id,claim_generation,2,'remote_initial_absent',?1,requested_name,scale_set_id,?1,?1,200,0,?2 FROM physical_attempt_jit_conflict_recovery_claims WHERE reservation_id=2",
        ("a".repeat(64), i64::try_from(receipt.observed_at_unix_ms())?),
    ).await.is_err(), "SQL accepted absence while the original registration can still complete");
    assert!(
        case.scope
            .record_jit_conflict_remote_initial_absence(&claim, &receipt)
            .await
            .is_err()
    );
    assert_eq!(scalar(&case.journal,
        "SELECT CAST(COUNT(*) AS TEXT) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=2 AND phase_index=2"
    ).await?, "0");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn upgrading_a_valid_v47_rejects_unfenced_interruption_but_preserves_409_absence()
-> TestResult<()> {
    Box::pin(upgrades_only_fenced_history()).await
}

async fn upgrades_only_fenced_history() -> TestResult<()> {
    for interrupted in [true, false] {
        let outcome = if interrupted {
            JitOutcome::Interrupted
        } else {
            JitOutcome::Conflict
        };
        let (case, claim) = Box::pin(prepared(Schema::Historical47, outcome)).await?;
        let receipt = empty_lookup(&claim)?;
        case.scope
            .record_jit_conflict_remote_initial_absence(&claim, &receipt)
            .await?;
        let case = case.reopen(Schema::Historical47).await?;
        let before = super::historical::proof_rows(&case.journal).await?;
        let opened = Box::pin(Journal::open(case.journal.path())).await;
        if interrupted {
            assert!(
                opened.is_err(),
                "upgrade accepted unfenced interrupted registration history"
            );
            assert_eq!(super::historical::version(&case.journal).await?, 47);
            assert_eq!(scalar(&case.journal,
                "SELECT CAST(COUNT(*) AS TEXT) FROM sqlite_schema WHERE name='jit_interrupted_remote_absence_guard'"
            ).await?, "0");
            assert_eq!(super::historical::proof_rows(&case.journal).await?, before);
            let case = case.reopen(Schema::Historical47).await?;
            assert_eq!(super::historical::proof_rows(&case.journal).await?, before);
        } else {
            drop(opened?);
            let case = case.reopen(Schema::Current).await?;
            assert_eq!(super::historical::proof_rows(&case.journal).await?, before);
            assert_eq!(super::historical::version(&case.journal).await?, 50);
            Box::pin(case.finish(&claim, false)).await?;
        }
    }
    Ok(())
}

async fn prepared(
    schema: Schema,
    outcome: JitOutcome,
) -> TestResult<(Case, JitConflictRecoveryClaim)> {
    let case = Box::pin(Case::new(schema, outcome, false)).await?;
    case.journal
        .recover_after_restart_without_authority()
        .await?;
    let candidates = case.scope.recoverable_jit_conflict_candidates().await?;
    let claim = case
        .scope
        .claim_jit_conflict_candidate(candidates.first().ok_or("cleanup candidate")?, 240)
        .await?
        .ok_or("cleanup claim")?;
    Box::pin(super::super::local::ensure_local_absence(
        &case.runtime()?,
        &case.docker,
        &case.scope,
        &case.engine,
        &claim,
    ))
    .await?;
    Ok((case, claim))
}

fn empty_lookup(
    claim: &JitConflictRecoveryClaim,
) -> TestResult<velnor_runner_github::RunnerResponseReceipt> {
    let mut transport = Github::new(claim.requested_name(), false);
    let (runner, receipt) = velnor_runner_github::observe_runner_by_name(
        &mut transport,
        claim.requested_name(),
        "admin",
    )?;
    assert!(runner.is_none());
    Ok(receipt)
}
