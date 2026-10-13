mod fixtures;

use velnor_actions_contract::canonical::digest_b3;
use velnor_actions_contract_workflow::{
    NamedCheckLaneVariant, ObligationDecision, PlanObligation, canonical_plan_digest,
};

use fixtures::{
    REPOSITORY, REPOSITORY_ID, fixture, lane_entry, outputs_for, provider_for, receipts_for,
    request,
};
use velnor_actions_orchestrator_merge_ports::{
    ScopedCompareError, ScopedCompareLane, ScopedCompareRequest, TaskReportOutputFanIn,
    bind_scoped_compare,
};

fn shared_job_fixture() -> fixtures::Fixture {
    let mut fixture = fixture();
    let task_id = "stack/mise/demo/format/default";
    let task_digest = digest_b3(b"second-task");
    let run_key = fixture.plan.run_key.clone();
    let hosted_job = fixture
        .plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.lane_variant == Some(NamedCheckLaneVariant::Hosted))
        .expect("hosted job in base fixture")
        .job_id
        .clone();
    let scale_set_job = fixture
        .plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.lane_variant == Some(NamedCheckLaneVariant::ScaleSet))
        .expect("Scale Set job in base fixture")
        .job_id
        .clone();
    fixture.plan.matrix.include.extend([
        lane_entry(
            &run_key,
            task_id,
            &task_digest,
            &hosted_job,
            NamedCheckLaneVariant::Hosted,
        ),
        lane_entry(
            &run_key,
            task_id,
            &task_digest,
            &scale_set_job,
            NamedCheckLaneVariant::ScaleSet,
        ),
    ]);
    fixture
        .plan
        .matrix
        .include
        .sort_by(|left, right| left.id.cmp(&right.id));
    fixture.plan.task_ids.push(task_id.to_owned());
    fixture.plan.task_ids.sort();
    fixture.plan.packages[0].tasks.push(task_id.to_owned());
    fixture.plan.packages[0].tasks.sort();
    fixture.plan.obligations.push(PlanObligation {
        task_id: task_id.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "changed".to_owned(),
        task_digest,
        input_digest: digest_b3(b"second-inputs"),
        closure_digest: digest_b3(b"second-closure"),
        baseline_proof: None,
    });
    fixture
        .plan
        .obligations
        .sort_by(|left, right| left.task_id.cmp(&right.task_id));
    fixture.plan.validate().expect("shared-job fixture plan");

    let head = fixture.plan.head.clone();
    let plan_digest = canonical_plan_digest(&fixture.plan).expect("shared-job plan digest");
    fixture.receipts = receipts_for(&fixture.plan, &head);
    fixture.outputs = outputs_for(&fixture.plan, &head, &plan_digest);
    fixture.provider = provider_for(&fixture.plan, &head);
    fixture
}

#[test]
fn binds_typed_lane_pair_to_exact_checkrun_artifact_and_rest_job_ids() {
    let fixture = fixture();
    let result = bind_scoped_compare(
        request("chainargos/JAVA-MONOREPO"),
        &fixture.plan,
        &fixture.receipts,
        &fixture.outputs,
        &fixture.provider,
    )
    .expect("scoped bindings");
    assert_eq!(result.repository_id, REPOSITORY_ID);
    assert_eq!(result.repository, "chainargos/java-monorepo");
    assert_eq!(result.lanes.len(), 2);
    let hosted = result
        .lanes
        .iter()
        .find(|lane| lane.lane == ScopedCompareLane::Hosted)
        .map(|lane| (lane.artifact_id, lane.check_run_id, lane.actions_job_id));
    let scale = result
        .lanes
        .iter()
        .find(|lane| lane.lane == ScopedCompareLane::ScaleSet)
        .map(|lane| (lane.artifact_id, lane.check_run_id, lane.actions_job_id));
    assert_eq!(hosted, Some((501, 601, 10_601)));
    assert_eq!(scale, Some((502, 602, 10_602)));
    assert_eq!(
        hosted.map(|(_, check_run_id, job_id)| check_run_id != job_id),
        Some(true)
    );
    assert_eq!(
        scale.map(|(_, check_run_id, job_id)| check_run_id != job_id),
        Some(true)
    );
}

#[test]
fn permits_multiple_task_receipts_for_one_workflow_job_key() {
    let fixture = shared_job_fixture();
    let result = bind_scoped_compare(
        request(REPOSITORY),
        &fixture.plan,
        &fixture.receipts,
        &fixture.outputs,
        &fixture.provider,
    )
    .expect("shared workflow job bindings");

    assert_eq!(result.lanes.len(), 4);
    let hosted: Vec<_> = result
        .lanes
        .iter()
        .filter(|lane| lane.lane == ScopedCompareLane::Hosted)
        .collect();
    assert_eq!(hosted.len(), 2);
    assert_eq!(hosted[0].workflow_job_key, hosted[1].workflow_job_key);
    assert_eq!(hosted[0].actions_job_id, hosted[1].actions_job_id);
    assert_ne!(hosted[0].task_id, hosted[1].task_id);
}

#[test]
fn rejects_check_run_alias_across_producer_keys_when_second_rest_job_is_missing() {
    let fixture = fixture();
    let mut raw = serde_json::to_value(&fixture.outputs).expect("sidecar value");
    let check_run_id = raw["producers"][0]["check_run_id"]
        .as_i64()
        .expect("positive fixture Check Run ID");
    raw["producers"][1]["check_run_id"] = serde_json::json!(check_run_id);
    let outputs = TaskReportOutputFanIn::parse_value(raw)
        .expect("internally-shaped sidecar with conflicting job identity");

    let mut provider = fixture.provider.clone();
    provider
        .jobs
        .retain(|job| job.check_run_id == Some(check_run_id));
    assert_eq!(provider.jobs.len(), 1);
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &outputs,
            &provider,
        ),
        Err(ScopedCompareError::InvalidProducerOutputs),
    );
}

#[test]
fn rejects_wrong_scope_plan_or_provider_attempt() {
    let fixture = fixture();
    assert_eq!(
        bind_scoped_compare(
            request("Other/java-monorepo"),
            &fixture.plan,
            &fixture.receipts,
            &fixture.outputs,
            &fixture.provider
        ),
        Err(ScopedCompareError::ProviderScopeMismatch),
    );
    let mut provider = fixture.provider.clone();
    provider.attempt += 1;
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &fixture.outputs,
            &provider
        ),
        Err(ScopedCompareError::ProviderScopeMismatch),
    );
    assert_eq!(
        bind_scoped_compare(
            ScopedCompareRequest {
                repository: REPOSITORY,
                run_id: 424_243,
                attempt: 2,
            },
            &fixture.plan,
            &fixture.receipts,
            &fixture.outputs,
            &fixture.provider
        ),
        Err(ScopedCompareError::PlanScopeMismatch),
    );
}

#[test]
fn rejects_missing_receipts_and_incomplete_selected_job_inventory() {
    let fixture = fixture();
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &[],
            &fixture.outputs,
            &fixture.provider
        ),
        Err(ScopedCompareError::MissingLaneInput),
    );
    let mut provider = fixture.provider.clone();
    provider.jobs.pop();
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &fixture.outputs,
            &provider
        ),
        Err(ScopedCompareError::JobBindingMismatch),
    );
}

#[test]
fn rejects_duplicate_receipts_and_missing_checkrun_or_artifact_rows() {
    let fixture = fixture();
    let mut duplicate_receipts = fixture.receipts.clone();
    duplicate_receipts.push(duplicate_receipts[0].clone());
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &duplicate_receipts,
            &fixture.outputs,
            &fixture.provider
        ),
        Err(ScopedCompareError::InvalidReceipt),
    );
    let mut provider = fixture.provider.clone();
    provider.jobs[0].check_run_id = None;
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &fixture.outputs,
            &provider
        ),
        Err(ScopedCompareError::JobBindingMismatch),
    );
    let mut provider = fixture.provider.clone();
    provider.artifacts[0].name.push_str("-foreign");
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &fixture.outputs,
            &provider
        ),
        Err(ScopedCompareError::ArtifactBindingMismatch),
    );
}

#[test]
fn rejects_source_checkrun_and_artifact_id_mismatches() {
    let fixture = fixture();
    let mut receipts = fixture.receipts.clone();
    receipts[0].source_sha = "cd".repeat(20);
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &receipts,
            &fixture.outputs,
            &fixture.provider
        ),
        Err(ScopedCompareError::InvalidReceipt),
    );

    let mut provider = fixture.provider.clone();
    provider.head_sha = "cd".repeat(20);
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &fixture.outputs,
            &provider
        ),
        Err(ScopedCompareError::ProviderScopeMismatch),
    );

    let mut raw = serde_json::to_value(&fixture.outputs).expect("sidecar value");
    raw["head_sha"] = serde_json::json!("cd".repeat(20));
    let outputs = TaskReportOutputFanIn::parse_value(raw).expect("well-formed sidecar");
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &outputs,
            &fixture.provider
        ),
        Err(ScopedCompareError::ProducerScopeMismatch),
    );

    let mut raw = serde_json::to_value(&fixture.outputs).expect("sidecar value");
    raw["producers"][0]["check_run_id"] = serde_json::json!(999);
    let outputs = TaskReportOutputFanIn::parse_value(raw).expect("well-formed sidecar");
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &outputs,
            &fixture.provider
        ),
        Err(ScopedCompareError::JobBindingMismatch),
    );

    let mut raw = serde_json::to_value(&fixture.outputs).expect("sidecar value");
    raw["producers"][0]["artifact_id"] = serde_json::json!(999);
    let outputs = TaskReportOutputFanIn::parse_value(raw).expect("well-formed sidecar");
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &outputs,
            &fixture.provider
        ),
        Err(ScopedCompareError::ArtifactBindingMismatch),
    );
}

#[test]
fn rejects_sidecar_scope_and_run_status_mismatches() {
    let fixture = fixture();
    let mut raw = serde_json::to_value(&fixture.outputs).expect("sidecar value");
    raw["run"]["repository_id"] = serde_json::json!("54321");
    let outputs = TaskReportOutputFanIn::parse_value(raw).expect("well-formed sidecar");
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &outputs,
            &fixture.provider
        ),
        Err(ScopedCompareError::ProducerScopeMismatch),
    );

    let mut provider = fixture.provider.clone();
    provider.run_conclusion = Some("failure".to_owned());
    assert_eq!(
        bind_scoped_compare(
            request(REPOSITORY),
            &fixture.plan,
            &fixture.receipts,
            &fixture.outputs,
            &provider
        ),
        Err(ScopedCompareError::ProviderRunUnsuccessful),
    );
}
