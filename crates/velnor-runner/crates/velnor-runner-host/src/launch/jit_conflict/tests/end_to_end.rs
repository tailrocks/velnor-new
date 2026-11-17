// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CRASH, JitOutcome, TestResult,
    case::{Case, FOREIGN, Schema},
    population, scalar,
};
use crate::journal::JitConflictRecoveryClaim;
use crate::launch::bootstrap::{UncertainRecovery, recover_uncertain_reservations};

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_claim_is_cleaned_by_name_and_releases_only_after_metadata_settlement()
-> TestResult<()> {
    for exists in [false, true] {
        Box::pin(cleanup(exists, false)).await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_claim_with_a_mismatched_owned_container_stays_charged() -> TestResult<()> {
    Box::pin(cleanup(true, true)).await
}

async fn cleanup(exists: bool, mismatch: bool) -> TestResult<()> {
    let case = Case::new(Schema::Current, JitOutcome::Interrupted, true).await?;
    assert_blocked(&case).await?;
    let case = case.reopen(Schema::Current).await?;
    case.journal
        .recover_after_restart_without_authority()
        .await?;
    assert_blocked(&case).await?;
    assert_repeat_claim_refused(&case).await?;
    let candidates = case.scope.recoverable_jit_conflict_candidates().await?;
    let claim = case
        .scope
        .claim_jit_conflict_candidate(
            candidates
                .first()
                .ok_or("interrupted claim has no cleanup owner")?,
            240,
        )
        .await?
        .ok_or("cleanup claim")?;
    recover_uncertain_reservations(&UncertainRecovery {
        docker: &case.docker,
        journal: &case.journal,
        scope: &case.scope,
        engine: &case.engine,
        images: &case.images,
        image_authority: &case.authority,
        pass: 0,
    })
    .await?;
    assert_eq!(case.fixture.objects(CRASH).len(), 8);
    let foreign = case.fixture.objects(FOREIGN);
    if mismatch {
        case.fixture
            .relabel_container(&CRASH.container_id("runner"));
    }
    let local = Box::pin(super::super::local::ensure_local_absence(
        &case.runtime()?,
        &case.docker,
        &case.scope,
        &case.engine,
        &claim,
    ))
    .await;
    if mismatch {
        assert!(local.is_err());
        assert!(
            case.fixture
                .objects(CRASH)
                .contains(&CRASH.container_id("runner"))
        );
        assert_eq!(case.fixture.objects(FOREIGN), foreign);
        assert_blocked(&case).await?;
        assert_eq!(
            scalar(
                &case.journal,
                "SELECT state FROM capacity_reservations WHERE id=2"
            )
            .await?,
            "uncertain"
        );
        return Ok(());
    }
    local.map_err(|error| {
        format!(
            "native local cleanup failed: {error}; requests={:?}",
            case.fixture.take_requests()
        )
    })?;
    assert_eq!(case.fixture.objects(CRASH), vec![CRASH.network_id()]);
    finish(case, &claim, exists, foreign).await
}

async fn finish(
    case: Case,
    claim: &JitConflictRecoveryClaim,
    exists: bool,
    foreign: Vec<String>,
) -> TestResult<()> {
    assert_blocked(&case).await?;
    case.remote_and_network(claim, exists).await?;
    assert_eq!(case.fixture.objects(CRASH), Vec::<String>::new());
    assert_eq!(case.fixture.objects(FOREIGN), foreign);
    assert_blocked(&case).await?;
    assert_eq!(
        scalar(
            &case.journal,
            "SELECT state FROM capacity_reservations WHERE id=2"
        )
        .await?,
        "uncertain"
    );
    case.settle().await?;
    let refreshed = population::plan(&case.journal, &case.scope, 2).await?;
    assert_eq!(refreshed.blocked, None);
    assert!(refreshed.actions.iter().any(|action| matches!(
        action,
        crate::host_controller::AssignedPopulationAction::ContinueExisting {
            reservation_id: 4,
            ..
        }
    )));
    let requests = case.fixture.take_requests();
    for role in ["runner", "dind"] {
        assert!(
            !requests
                .iter()
                .any(|request| request.contains(&format!("name=w{}-{role}", CRASH.nonce())))
        );
        assert!(!requests.iter().any(|request| {
            request.contains(&format!("/containers/{}/start", CRASH.container_id(role)))
        }));
    }
    case.journal
        .recover_after_restart_without_authority()
        .await?;
    assert!(
        case.scope
            .recoverable_jit_conflict_candidates()
            .await?
            .is_empty()
    );
    let case = case.reopen(Schema::Current).await?;
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

async fn assert_blocked(case: &Case) -> TestResult<()> {
    let plan = population::plan(&case.journal, &case.scope, 2).await?;
    assert!(plan.blocked.is_some());
    assert_eq!(plan.actions, Vec::new());
    Ok(())
}

async fn assert_repeat_claim_refused(case: &Case) -> TestResult<()> {
    let policy = scalar(
        &case.journal,
        "SELECT policy_digest FROM connection_active_job_authority LIMIT 1",
    )
    .await?;
    assert!(
        case.scope
            .claim_runner_jit_for_request(1, 2, 2, &format!("v{}", CRASH.nonce()), 1, &policy)
            .await?
            .is_none()
    );
    Ok(())
}
