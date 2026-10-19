//! Focused tests for fresh schema-2 baseline publication proofs.

use std::collections::BTreeMap;

use super::*;
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, JobDisposition, MatrixEntry, NamedCheckLaneVariant,
    ObligationDecision, Plan, PlanGenerator, PlanJob, PlanMatrix, PlanObligation, PlanRunner,
    PlannedPlatform, RunnerSelection, Trust, plan_id_for_run,
};

fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

pub(super) fn fixture_plan(head: &str, run_key: &str) -> Plan {
    let task_id = "stack/rust/demo/test/default";
    let task_digest = digest(1);
    let job_id = "rust-demo";
    let mut entry = MatrixEntry::derive(
        "rust",
        task_id,
        "cargo test",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "test".to_owned(),
                ExecuteTaskRef::Single(task_id.to_owned()),
            )]),
        },
        &digest(2),
        run_key,
        job_id,
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("platform"),
    )
    .expect("matrix entry");
    let toolchain_inputs = velnor_actions_contract::cachekey::ToolchainInputs {
        tools: vec!["cargo@1.99.1".to_owned()],
        components: Vec::new(),
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo-test".to_owned(),
    };
    let toolchain_id = velnor_actions_contract::cachekey::toolchain_id(&toolchain_inputs)
        .expect("toolchain identity");
    entry.toolchain_id = Some(toolchain_id);
    entry.toolchain_inputs = Some(toolchain_inputs);
    let plan = Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.to_owned(),
        plan_id: plan_id_for_run(run_key).expect("plan id"),
        base: Some("b".repeat(40)),
        head: head.to_owned(),
        event: WorkflowEvent::Push,
        qualification: None,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Trusted,
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![PlanObligation {
            task_id: task_id.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "affected_by_change".to_owned(),
            task_digest: task_digest.clone(),
            task_binding_digest: velnor_actions_contract::TaskBindingDigest::new(
                task_digest.clone(),
            )
            .expect("binding digest"),
            input_digest: digest(2),
            closure_digest: digest(3),
            graph_digest: digest(4),
            mbx_digest: digest(5),
            profile: "default".to_owned(),
            job_id: job_id.to_owned(),
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![task_id.to_owned()],
        jobs: vec![PlanJob {
            job_id: job_id.to_owned(),
            disposition: JobDisposition::Run,
        }],
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    plan.validate().expect("valid plan");
    plan
}

fn paired_fixture_plan() -> Plan {
    let mut plan = fixture_plan(&"a".repeat(40), "r7-a1");
    let original = &plan.matrix.include[0];
    let mut hosted = MatrixEntry::derive_for_lane(
        &original.stack_id,
        &original.task_id,
        &original.run,
        &original.task_digest,
        original.adapter_metadata.clone(),
        original.execute_task_ids.clone(),
        &original.input_digest,
        &plan.run_key,
        "rust-demo__hosted",
        Some(NamedCheckLaneVariant::Hosted),
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("hosted platform"),
    )
    .expect("hosted entry");
    let mut local = MatrixEntry::derive_for_lane(
        &original.stack_id,
        &original.task_id,
        &original.run,
        &original.task_digest,
        original.adapter_metadata.clone(),
        original.execute_task_ids.clone(),
        &original.input_digest,
        &plan.run_key,
        "rust-demo__local",
        Some(NamedCheckLaneVariant::ScaleSet),
        PlannedPlatform::new(
            "scale-set:velnor+ubuntu-26.04-scale-set",
            "x86_64-unknown-linux-gnu",
        )
        .expect("scale-set platform"),
    )
    .expect("local entry");
    for entry in [&mut hosted, &mut local] {
        entry.toolchain_id = original.toolchain_id.clone();
        entry.toolchain_inputs = original.toolchain_inputs.clone();
    }
    plan.matrix.include = vec![hosted, local];
    plan.obligations[0].job_id = "rust-demo__hosted".to_owned();
    plan.jobs = ["rust-demo__hosted", "rust-demo__local"]
        .into_iter()
        .map(|job_id| PlanJob {
            job_id: job_id.to_owned(),
            disposition: JobDisposition::Run,
        })
        .collect();
    plan.validate().expect("paired plan");
    plan
}

pub(super) fn request_json(head: &str) -> String {
    serde_json::json!({
        "schema": 1, "op": PUBLISH_OP, "event": "push",
        "base": "b".repeat(40), "head": head, "repository": "o/r",
        "git_ref": "refs/heads/testmain", "default_branch": "testmain"
    })
    .to_string()
}

pub(super) fn refuse_problem(request: &str, _plan: &Plan, _run_key: &str) -> String {
    let request = publish_request(request).expect("valid request");
    publish_gate(&request)
        .expect_err("request must refuse")
        .to_string()
}

fn manifest_json(
    request: &PublishRequest,
    plan: &Plan,
    reports: &[velnor_actions_contract::TaskReport],
) -> Result<serde_json::Value, OrchestratorError> {
    let (bytes, _) = manifest::build_manifest(request, plan, 7, 1, reports)?;
    serde_json::from_slice(&bytes).map_err(|_| internal("test_manifest_decode"))
}

#[test]
fn schema_two_manifest_has_fresh_proof_bound_to_successful_report() {
    let plan = fixture_plan(&"a".repeat(40), "r7-a1");
    let entry = &plan.matrix.include[0];
    let task = &plan.obligations[0];
    let report = crate::task_report::terminal_task_report(&plan, entry, &task.task_digest, 0, None)
        .expect("successful report");
    let request = publish_request(&request_json(&plan.head)).expect("request");
    let manifest = manifest_json(&request, &plan, &[report]).expect("manifest");

    assert_eq!(manifest["schema"], 2);
    assert_eq!(manifest["tasks"].as_array().expect("tasks").len(), 1);
    let proof = &manifest["tasks"][0]["proof"];
    assert_eq!(proof["task_id"], task.task_id);
    assert_eq!(proof["graph_digest"], task.graph_digest);
    assert_eq!(proof["mbx_digest"], task.mbx_digest);
    assert_eq!(proof["profile"], task.profile);
    assert_eq!(proof["proof_run_id"], 7);
}

#[test]
fn publisher_rejects_failed_and_missing_task_reports() {
    let plan = fixture_plan(&"a".repeat(40), "r7-a1");
    let entry = &plan.matrix.include[0];
    let task = &plan.obligations[0];
    let failed = crate::task_report::terminal_task_report(&plan, entry, &task.task_digest, 1, None)
        .expect("failed report");
    let request = publish_request(&request_json(&plan.head)).expect("request");
    assert!(manifest_json(&request, &plan, &[failed]).is_err());
    assert!(manifest_json(&request, &plan, &[]).is_err());
}

#[test]
fn paired_obligation_requires_and_validates_each_matrix_lane_report() {
    let plan = paired_fixture_plan();
    let reports: Vec<_> = plan
        .matrix
        .include
        .iter()
        .map(|entry| {
            crate::task_report::terminal_task_report(
                &plan,
                entry,
                &plan.obligations[0].task_digest,
                0,
                None,
            )
            .expect("successful lane report")
        })
        .collect();
    let request = publish_request(&request_json(&plan.head)).expect("request");
    let manifest = manifest_json(&request, &plan, &reports).expect("paired manifest");
    assert_eq!(manifest["tasks"].as_array().expect("tasks").len(), 1);
    assert_eq!(
        manifest["tasks"][0]["proof"]["platform_id"],
        plan.matrix.include[0].planned_platform.platform_id,
    );

    assert!(manifest_json(&request, &plan, &reports[..1]).is_err());
    let mut failed_local = reports;
    failed_local[1].exit_code = 1;
    assert!(manifest_json(&request, &plan, &failed_local).is_err());
}

#[test]
fn generic_request_writer_dispatches_baseline_publish_operation() {
    let head = "a".repeat(40);
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join(format!("{PUBLISH_OP}-request.json"));
    let payload = serde_json::json!({
        "ref": "refs/heads/testmain",
        "before": "b".repeat(40),
        "after": head,
        "repository": {"default_branch": "testmain"}
    })
    .to_string();
    crate::internal_request::write_request_parts(
        &path,
        "push",
        &payload,
        Some(&head),
        Some("o/r"),
        temp.path(),
    )
    .expect("publish request route");

    let bytes = std::fs::read_to_string(path).expect("request bytes");
    let request: PublishRequest = publish_request(&bytes).expect("publish envelope");
    assert_eq!(request.op.as_deref(), Some(PUBLISH_OP));
    assert_eq!(request.head, head);
    assert_eq!(request.default_branch.as_deref(), Some("testmain"));
}

#[test]
fn publish_request_rejects_wrong_schema_and_operation() {
    let head = "a".repeat(40);
    for (field, value) in [
        ("schema", serde_json::json!(2)),
        ("op", serde_json::json!("merge-v1")),
    ] {
        let mut request: serde_json::Value =
            serde_json::from_str(&request_json(&head)).expect("json");
        request[field] = value;
        assert!(publish_request(&request.to_string()).is_err());
    }
}

#[path = "baseline_publish_request_tests.rs"]
mod request_tests;
