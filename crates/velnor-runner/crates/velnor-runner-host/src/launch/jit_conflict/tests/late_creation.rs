// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CRASH, JitOutcome, TestResult,
    case::{Case, FOREIGN, Schema},
    github::Github,
    population, scalar,
};
use crate::{error::HostError, launch::jit_conflict::ReconcileContext, listen::Secret};

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_jit_stays_charged_until_its_delayed_registration_is_removed()
-> TestResult<()> {
    Box::pin(delayed_registration()).await
}

async fn delayed_registration() -> TestResult<()> {
    let case = Case::new(Schema::Current, JitOutcome::Interrupted, true).await?;
    let case = case.reopen(Schema::Current).await?;
    case.journal
        .recover_after_restart_without_authority()
        .await?;
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
    for _ in 0..2 {
        assert!(
            !super::super::remote::reconcile_admin(
                &context,
                &claim,
                &mut identity,
                &mut admin,
                &Secret::new("admin"),
            )
            .await?,
            "an empty lookup cannot settle an interrupted registration request"
        );
    }
    assert!(admin.lookups.len() >= 2);
    assert_retained(&case).await?;
    admin.runners.insert(21, (name.clone(), 7));
    assert!(
        super::super::remote::reconcile_admin(
            &context,
            &claim,
            &mut identity,
            &mut admin,
            &Secret::new("admin"),
        )
        .await?
    );
    assert!(!admin.runners.contains_key(&21));
    assert!(admin.runners.contains_key(&99));
    assert!(admin.lookups.iter().all(|value| value == &name));
    super::super::ensure_network_absence(&case.docker, &case.scope, &case.engine, &claim).await?;
    case.settle().await?;
    assert_released(case, foreign).await
}

async fn assert_retained(case: &Case) -> TestResult<()> {
    assert_eq!(scalar(&case.journal,
        "SELECT COUNT(*) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=2 AND phase_index IN (2,4)"
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
            "SELECT COUNT(*) FROM physical_attempt_current WHERE reservation_id=2"
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
