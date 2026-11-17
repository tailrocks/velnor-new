// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CRASH, JitOutcome, TestResult, binding,
    case::{Case, FOREIGN, Schema},
    population, scalar, seed,
};

#[path = "inbox_fake.rs"]
mod fake;

#[tokio::test(flavor = "multi_thread")]
async fn draining_a_queued_completion_unlocks_interrupted_jit_cleanup_without_another_restart()
-> TestResult<()> {
    if fake::isolated(concat!(
        module_path!(),
        "::draining_a_queued_completion_unlocks_interrupted_jit_cleanup_without_another_restart"
    ))? {
        return Ok(());
    }
    let case = Case::new(Schema::Current, JitOutcome::Interrupted, true).await?;
    seed_unrelated_request(&case).await?;
    let body = lifecycle("JobCompleted", 50, false);
    case.scope.store_completion_inbox(80, &body).await?;
    let owned = case.fixture.objects(CRASH);
    let foreign = case.fixture.objects(FOREIGN);
    let case = case.reopen(Schema::Current).await?;
    case.journal
        .recover_after_restart_without_authority()
        .await?;
    let classified = super::shape(&case.journal).await?;
    assert_waiting(&case, &owned, &foreign).await?;
    fake::poll(&case, &body).await?;
    assert_eq!(inbox_count(&case).await?, "0");
    assert_eq!(
        scalar(
            &case.journal,
            "SELECT CAST(count(*) AS TEXT) FROM connection_request_authorizations WHERE request_id=50 AND completed_message_id=80 AND completed_event_digest IS NOT NULL AND completed_job_id='job-50' AND completed_result='success' AND completed_runner_id IS NULL AND completed_runner_name IS NULL AND completed_intent_id IS NULL"
        )
        .await?,
        "1"
    );
    let candidates = case.scope.recoverable_jit_conflict_candidates().await?;
    let candidate = candidates.first().ok_or(
        "drained completion inbox did not unlock interrupted JIT cleanup without another restart",
    )?;
    assert_eq!(
        classified,
        "uncertain/uncertain/jit_uncertain/uncertain/uncertain"
    );
    let claim = case
        .scope
        .claim_jit_conflict_candidate(candidate, 240)
        .await?
        .ok_or("cleanup claim after inbox drain")?;
    super::super::local::ensure_local_absence(
        &case.runtime()?,
        &case.docker,
        &case.scope,
        &case.engine,
        &claim,
    )
    .await?;
    assert_eq!(case.fixture.objects(CRASH), vec![CRASH.network_id()]);
    assert_blocked(&case).await?;
    case.remote_and_network(&claim, true).await?;
    assert_eq!(case.fixture.objects(CRASH), Vec::<String>::new());
    assert_blocked(&case).await?;
    case.settle().await?;
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
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn same_requested_name_events_stay_deferred_and_cannot_authorize_unbound_jit_cleanup()
-> TestResult<()> {
    if fake::isolated(concat!(
        module_path!(),
        "::same_requested_name_events_stay_deferred_and_cannot_authorize_unbound_jit_cleanup"
    ))? {
        return Ok(());
    }
    for kind in ["JobStarted", "JobCompleted"] {
        let case = Case::new(Schema::Current, JitOutcome::Interrupted, true).await?;
        let body = lifecycle(kind, 2, true);
        case.scope.store_completion_inbox(80, &body).await?;
        let owned = case.fixture.objects(CRASH);
        let foreign = case.fixture.objects(FOREIGN);
        let case = case.reopen(Schema::Current).await?;
        case.journal
            .recover_after_restart_without_authority()
            .await?;
        fake::poll(&case, &body).await?;
        assert_eq!(inbox_count(&case).await?, "1");
        assert_eq!(
            scalar(
                &case.journal,
                "SELECT CAST(attempts AS TEXT) FROM connection_completion_inbox WHERE message_id=80"
            )
            .await?,
            "1"
        );
        assert_eq!(scalar(&case.journal,"SELECT CAST(count(*) AS TEXT) FROM connection_request_authorizations WHERE request_id=2 AND started_event_digest IS NULL AND completed_event_digest IS NULL").await?, "1");
        assert_waiting(&case, &owned, &foreign).await?;
    }
    Ok(())
}

async fn assert_waiting(case: &Case, owned: &[String], foreign: &[String]) -> TestResult<()> {
    assert!(
        case.scope
            .recoverable_jit_conflict_candidates()
            .await?
            .is_empty()
    );
    assert_blocked(case).await?;
    assert_eq!(case.fixture.objects(CRASH), owned);
    assert_eq!(case.fixture.objects(FOREIGN), foreign);
    assert_eq!(scalar(&case.journal,"SELECT CAST(count(*) AS TEXT) FROM capacity_reservations WHERE id=2 AND state IN ('reserved','uncertain') AND local_cleanup_proven=0 AND remote_cleanup_proven=0").await?, "1");
    Ok(())
}

async fn assert_blocked(case: &Case) -> TestResult<()> {
    let plan = population::plan(&case.journal, &case.scope, 2).await?;
    assert!(plan.blocked.is_some());
    assert!(plan.actions.is_empty());
    Ok(())
}

async fn inbox_count(case: &Case) -> TestResult<String> {
    scalar(
        &case.journal,
        "SELECT CAST(count(*) AS TEXT) FROM connection_completion_inbox",
    )
    .await
}

fn lifecycle(kind: &str, request: i64, runner: bool) -> String {
    let mut job = serde_json::json!({"messageType":kind,"runnerRequestId":request,"jobId":format!("job-{request}"),"ownerName":"owner","repositoryName":"repo","jobWorkflowRef":"owner/repo/.github/workflows/ci.yml@refs/heads/main","workflowRunId":1,"eventName":"push","result":"success"});
    if runner {
        job["runnerId"] = serde_json::json!(21);
        job["runnerName"] = serde_json::json!(format!("v{}", CRASH.nonce()));
    }
    serde_json::json!([job]).to_string()
}

async fn seed_unrelated_request(case: &Case) -> TestResult<()> {
    let response = r#"{"kind":"returned","request_ids":[50]}"#;
    let digest = binding::digest(response.as_bytes());
    seed::execute(&case.journal, &format!("INSERT INTO connection_request_authorizations(connection_id,scale_set_id,request_id,message_id,policy_mode,authority_model,binding_digest,source_digest,source_json,acquire_state,acquire_response_json,acquire_response_digest) SELECT connection_id,scale_set_id,50,50,policy_mode,authority_model,binding_digest,source_digest,source_json,'acquired','{response}','{digest}' FROM connection_request_authorizations WHERE request_id=2")).await
}
