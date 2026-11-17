// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CRASH, JitOutcome, TestResult,
    case::{Case, FOREIGN, Schema},
    github::Github,
    population, scalar,
};
use crate::{error::HostError, launch::jit_conflict::ReconcileContext, listen::Secret};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

#[tokio::test(flavor = "multi_thread")]
async fn an_undispatched_jit_releases_at_its_deadline_and_quarantines_late_registration()
-> TestResult<()> {
    Box::pin(delayed_registration(false)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_before_the_post_commit_stamp_gets_one_bounded_recovery_deadline() -> TestResult<()>
{
    Box::pin(delayed_registration(true)).await
}

async fn delayed_registration(missing_stamp: bool) -> TestResult<()> {
    let (mut case, original_commit_time, commit_observed_at) =
        Box::pin(restarted_case(missing_stamp)).await?;
    let candidates = case.scope.recoverable_jit_conflict_candidates().await?;
    let claim = case
        .scope
        .claim_jit_conflict_candidate(
            candidates.first().ok_or("interrupted cleanup candidate")?,
            240,
        )
        .await?
        .ok_or("interrupted cleanup claim")?;
    super::super::local::ensure_local_absence(
        &case.runtime()?,
        &case.docker,
        &case.scope,
        &case.engine,
        &claim,
    )
    .await?;
    let foreign = case.fixture.objects(FOREIGN);
    let name = format!("v{}", CRASH.nonce());
    let mut admin = Github::new(&name, false);
    let mut identity = Github::new(&name, false);
    let mut link = admin.link(&case.binding)?;
    let pat = Secret::new("pat");
    let context = ReconcileContext {
        docker: &case.docker,
        journal: &case.journal,
        scope: &case.scope,
        binding: &case.binding,
        set_id: 7,
        link: &mut link,
        github_pat: &pat,
        selected: &case.engine,
        images: &case.images,
        image_authority: &case.authority,
    };
    let clock = Arc::new(AtomicI64::new(commit_observed_at + 59));
    admin.lookup_clock = Some((Arc::clone(&clock), commit_observed_at + 60));
    for _ in 0..2 {
        clock.store(commit_observed_at + 59, Ordering::SeqCst);
        assert!(
            !super::super::remote::reconcile_admin_with_clock(
                &context,
                &claim,
                &mut identity,
                &mut admin,
                &Secret::new("admin"),
                || Ok(clock.load(Ordering::SeqCst)),
            )
            .await.map_err(|error| format!("pre-deadline recovery after claim commit (post-commit stamp missing={missing_stamp}) failed: {error}"))?,
            "an empty lookup cannot settle an interrupted registration request"
        );
    }
    assert!(admin.lookups.len() >= 2);
    assert_retained(&case).await?;
    assert!(
        super::super::remote::reconcile_admin_with_clock(
            &context,
            &claim,
            &mut identity,
            &mut admin,
            &Secret::new("admin"),
            || Ok(commit_observed_at + 60),
        )
        .await?
    );
    assert!(!admin.runners.contains_key(&21));
    assert!(admin.runners.contains_key(&99));
    assert!(admin.lookups.iter().all(|value| value == &name));
    super::super::ensure_network_absence(&case.docker, &case.scope, &case.engine, &claim).await?;
    case.settle().await?;
    assert!(commit_observed_at >= original_commit_time);
    assert_late_runner_quarantined(&mut case, &mut admin, name).await?;
    assert_released(case, foreign).await
}

async fn restarted_case(missing_stamp: bool) -> TestResult<(Case, i64, i64)> {
    let case = Box::pin(Case::new(Schema::Current, JitOutcome::Interrupted, true)).await?;
    let original_commit_time = claim_time(&case).await?;
    if missing_stamp {
        super::seed::unguarded(
            &case.journal,
            &[
                "UPDATE jit_dispatch_claim_times SET commit_observed_at=NULL WHERE intent_id=1"
                    .to_owned(),
            ],
        )
        .await?;
    }
    let mut case = case.reopen(Schema::Current).await?;
    case.journal
        .recover_after_restart_without_authority()
        .await?;
    let commit_observed_at = claim_time(&case).await?;
    if !missing_stamp {
        assert_eq!(commit_observed_at, original_commit_time);
    }
    case = case.reopen(Schema::Current).await?;
    case.journal
        .recover_after_restart_without_authority()
        .await?;
    assert_eq!(claim_time(&case).await?, commit_observed_at);
    Ok((case, original_commit_time, commit_observed_at))
}

async fn assert_late_runner_quarantined(
    case: &mut Case,
    admin: &mut Github,
    name: String,
) -> TestResult<()> {
    admin.runners.insert(21, (name.clone(), 7));
    assert_eq!(
        scalar(
            &case.journal,
            "SELECT state FROM connection_runner_jits WHERE intent_id=1"
        )
        .await?,
        "conflict_retired"
    );
    assert_eq!(
        case.scope
            .bind_jit_runner(
                case.jit_claim.take().ok_or("original claim")?,
                &velnor_runner_github::RunnerReference {
                    id: 21,
                    name,
                    runner_scale_set_id: 7
                }
            )
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(scalar(&case.journal, "SELECT publication_state||'/'||delivery_state FROM capacity_reservation_provisioning_state WHERE reservation_id=2").await?, "not_requested/not_requested");
    Ok(())
}

async fn claim_time(case: &Case) -> TestResult<i64> {
    Ok(scalar(
        &case.journal,
        "SELECT CAST(COALESCE(commit_observed_at,0) AS TEXT) FROM jit_dispatch_claim_times WHERE intent_id=1",
    )
    .await?
    .parse()?)
}

async fn assert_retained(case: &Case) -> TestResult<()> {
    assert_eq!(scalar(&case.journal,
        "SELECT CAST(COUNT(*) AS TEXT) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=2 AND phase_index IN (2,4)"
    ).await?, "0");
    assert_eq!(
        scalar(
            &case.journal,
            "SELECT state||'/'||remote_cleanup_proven FROM capacity_reservations WHERE id=2"
        )
        .await?,
        "uncertain/0"
    );
    assert_eq!(
        case.runtime()?
            .settle_storage_metadata_after_cleanup(&case.scope, 2)
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(
        scalar(
            &case.journal,
            "SELECT CAST(COUNT(*) AS TEXT) FROM physical_attempt_current WHERE reservation_id=2"
        )
        .await?,
        "1"
    );
    assert!(
        population::plan(&case.journal, &case.scope, 2)
            .await?
            .blocked
            .is_some()
    );
    Ok(())
}

async fn assert_released(case: Case, foreign: Vec<String>) -> TestResult<()> {
    assert_eq!(
        scalar(
            &case.journal,
            "SELECT state FROM capacity_reservations WHERE id=2"
        )
        .await?,
        "released"
    );
    let plan = population::plan(&case.journal, &case.scope, 2).await?;
    assert_eq!(plan.blocked, None);
    assert!(plan.actions.iter().any(|action| matches!(
        action,
        crate::host_controller::AssignedPopulationAction::ContinueExisting {
            reservation_id: 4,
            ..
        }
    )));
    assert_eq!(case.fixture.objects(FOREIGN), foreign);
    let case = case.reopen(Schema::Current).await?;
    assert!(
        case.scope
            .recoverable_jit_conflict_candidates()
            .await?
            .is_empty()
    );
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
