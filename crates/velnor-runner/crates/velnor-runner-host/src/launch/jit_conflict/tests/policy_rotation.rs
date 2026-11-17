// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CRASH, JitOutcome, Pair, TestResult, binding,
    case::{Case, FOREIGN, Schema},
    population, scalar,
};
use crate::config::{ApprovedJobAuthorityPolicy, HostConfig};
use crate::journal::JitConflictRecoveryClaim;

pub(super) const CONTROL: Pair = Pair {
    reservation: 6,
    bound: false,
};

#[tokio::test(flavor = "multi_thread")]
async fn policy_rotation_before_cleanup_preserves_interrupted_recovery() -> TestResult<()> {
    Box::pin(recovers(Schema::Current, JitOutcome::Interrupted, false)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn policy_rotation_before_cleanup_preserves_409_recovery() -> TestResult<()> {
    Box::pin(recovers(Schema::Current, JitOutcome::Conflict, false)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn policy_rotation_during_interrupted_cleanup_preserves_local_proof_and_settlement()
-> TestResult<()> {
    Box::pin(recovers(Schema::Current, JitOutcome::Interrupted, true)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn policy_rotation_during_409_cleanup_preserves_local_proof_and_settlement() -> TestResult<()>
{
    Box::pin(recovers(Schema::Current, JitOutcome::Conflict, true)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn historical_46_policy_rotation_preserves_interrupted_local_proof_after_upgrade()
-> TestResult<()> {
    Box::pin(recovers(
        Schema::Historical46,
        JitOutcome::Interrupted,
        true,
    ))
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn historical_46_policy_rotation_preserves_409_local_proof_after_upgrade() -> TestResult<()> {
    Box::pin(recovers(Schema::Historical46, JitOutcome::Conflict, true)).await
}

async fn recovers(schema: Schema, outcome: JitOutcome, during: bool) -> TestResult<()> {
    let case = Case::with_control(schema, outcome, true, true).await?;
    case.journal
        .recover_after_restart_without_authority()
        .await?;
    let foreign = case.fixture.objects(FOREIGN);
    let before = frozen(&case).await?;
    let claim = if during {
        let claim = claim(&case).await?;
        local(&case, &claim).await?;
        rotate(&case).await?;
        assert_current_only(&case).await?;
        claim
    } else {
        rotate(&case).await?;
        assert_current_only(&case).await?;
        claim(&case).await?
    };
    let persisted = super::historical::proof_rows(&case.journal).await?;
    let case = case.reopen(Schema::Current).await?;
    assert_eq!(
        super::historical::proof_rows(&case.journal).await?,
        persisted
    );
    assert_eq!(frozen(&case).await?, before);
    case.scope
        .validate_jit_conflict_claim(&claim)
        .await
        .map_err(|error| {
            format!("historical live cleanup claim refused after policy rotation: {error}")
        })?;
    local(&case, &claim).await?;
    assert_eq!(case.fixture.objects(CRASH), vec![CRASH.network_id()]);
    finish(case, &claim, foreign, before).await
}

async fn finish(
    case: Case,
    claim: &JitConflictRecoveryClaim,
    foreign: Vec<String>,
    before: String,
) -> TestResult<()> {
    assert!(
        population::plan(&case.journal, &case.scope, 3)
            .await?
            .blocked
            .is_some()
    );
    case.remote_and_network(claim, true).await?;
    assert_eq!(
        scalar(
            &case.journal,
            "SELECT state FROM capacity_reservations WHERE id=2"
        )
        .await?,
        "uncertain"
    );
    assert!(
        population::plan(&case.journal, &case.scope, 3)
            .await?
            .blocked
            .is_some()
    );
    case.settle().await?;
    let plan = population::plan(&case.journal, &case.scope, 3).await?;
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
    assert_eq!(frozen(&case).await?, before);
    assert_eq!(super::historical::version(&case.journal).await?, 49);
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

async fn claim(case: &Case) -> TestResult<JitConflictRecoveryClaim> {
    let candidates = case
        .scope
        .recoverable_jit_conflict_candidates()
        .await
        .map_err(|error| {
            format!("historical cleanup candidate lookup after policy rotation: {error}")
        })?;
    Ok(case
        .scope
        .claim_jit_conflict_candidate(
            candidates
                .first()
                .ok_or("historical receipt lost cleanup eligibility after policy rotation")?,
            240,
        )
        .await?
        .ok_or("historical cleanup claim")?)
}

async fn local(case: &Case, claim: &JitConflictRecoveryClaim) -> TestResult<()> {
    super::super::local::ensure_local_absence(
        &case.runtime()?,
        &case.docker,
        &case.scope,
        &case.engine,
        claim,
    )
    .await?;
    Ok(())
}

async fn rotate(case: &Case) -> TestResult<()> {
    let old: ApprovedJobAuthorityPolicy = serde_json::from_str(
        &scalar(
            &case.journal,
            "SELECT policy_json FROM connection_job_authority_policies WHERE generation=1",
        )
        .await?,
    )?;
    assert!(
        old.workflow_content_sha256(42, ".github/workflows/ci.yml")
            .is_some()
    );
    let mut value = serde_json::to_value(&old)?;
    value["generation"] = serde_json::json!(2);
    value["workflows"][0]["path"] = serde_json::json!(".github/workflows/replacement.yml");
    let unsigned: ApprovedJobAuthorityPolicy = serde_json::from_value(value.clone())?;
    value["policy_sha256"] = serde_json::json!(binding::digest(&unsigned.canonical_json()?));
    let revised: ApprovedJobAuthorityPolicy = serde_json::from_value(value.clone())?;
    assert!(
        revised
            .workflow_content_sha256(42, ".github/workflows/ci.yml")
            .is_none()
    );
    let config: HostConfig = serde_json::from_value(serde_json::json!({
        "schema": 2, "connections": [case.binding], "job_authority_policies": [value],
        "host": {"max_jobs": 4, "resources": {"runner_cpu_millicores": 2000, "runner_memory_bytes": 4_294_967_296_u64,
        "dind_cpu_millicores":1000,"dind_memory_bytes":2_147_483_648_u64,"operational_memory_reserve_bytes":1_073_741_824_u64}},
        "docker":{"context":"fixture","platform":"linux/amd64","endpoint":"unix:///fixture.sock"}
    }))?;
    case.scope
        .install_job_authority_policy(&config)
        .await
        .map_err(|error| format!("install materially changed active policy: {error}"))?;
    assert_eq!(
        scalar(
            &case.journal,
            "SELECT CAST(generation AS TEXT) FROM connection_active_job_authority"
        )
        .await?,
        "2"
    );
    Ok(())
}

async fn assert_current_only(case: &Case) -> TestResult<()> {
    let old = scalar(
        &case.journal,
        "SELECT policy_digest FROM connection_job_authority_policies WHERE generation=1",
    )
    .await?;
    let current = scalar(
        &case.journal,
        "SELECT policy_digest FROM connection_active_job_authority",
    )
    .await?;
    let name = format!("v{}", CONTROL.nonce());
    assert!(
        case.scope
            .claim_runner_jit_for_request(5, 6, 6, &name, 1, &old)
            .await?
            .is_none()
    );
    let claim = case
        .scope
        .claim_runner_jit_for_request(5, 6, 6, &name, 2, &current)
        .await?
        .ok_or("current policy rejected fresh eligible JIT claim")?;
    case.scope
        .bind_jit_runner(
            claim,
            &velnor_runner_github::RunnerReference {
                id: 23,
                name,
                runner_scale_set_id: 7,
            },
        )
        .await?;
    Ok(())
}

async fn frozen(case: &Case) -> TestResult<String> {
    scalar(&case.journal, "SELECT authority_generation||'/'||authority_digest||'/'||binding_digest||'/'||source_digest||'/'||acquire_response_digest FROM physical_attempt_jit_recovery_sources WHERE reservation_id=2").await
}
