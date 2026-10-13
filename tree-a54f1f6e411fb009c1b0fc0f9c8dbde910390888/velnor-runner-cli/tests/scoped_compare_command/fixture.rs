//! Valid plan, runtime receipts, producer outputs, and provider-read fixture.
#![expect(
    clippy::expect_used,
    reason = "fixture construction expectations identify which validated contract input broke"
)]

use serde_json::{Value, json};
use velnor_actions_contract::{digest_b3, ids::plan_id_for_run, task_report_id_for_task};
use velnor_actions_contract_workflow::{
    ArtifactBuildRunContext, ExecuteTaskIds, MatrixEntry, NamedCheckLaneVariant,
    ObligationDecision, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage,
    PlanRunner, TaskRuntimeIdentity, TaskRuntimeReceipt, Trust, WorkflowEvent,
    canonical_plan_digest,
};
use velnor_actions_orchestrator_merge_ports::TaskReportOutputFanIn;

pub(super) const REPOSITORY: &str = "ChainArgos/java-monorepo";
pub(super) const RUN_ID: i64 = 424_242;
pub(super) const ATTEMPT: u32 = 2;
const REPOSITORY_ID: i64 = 12_345;
const HEAD: &str = "abababababababababababababababababababab";

pub(super) fn scoped_input() -> Value {
    let plan = make_plan();
    let receipts = make_receipts(&plan);
    let plan_digest = canonical_plan_digest(&plan).expect("plan digest");
    let outputs = make_producer_outputs(&plan, &plan_digest);
    json!({
        "schema": 1,
        "plan": plan,
        "receipts": receipts,
        "producer_outputs": outputs,
        "provider_read": {
            "kind": "complete",
            "value": make_provider_evidence(&plan)
        }
    })
}

fn make_plan() -> Plan {
    let run_key = format!("r{RUN_ID}-a{ATTEMPT}");
    let task_id = "stack/mise/demo/check/default";
    let task_digest = digest_b3(b"task");
    let mut entries = [
        lane_entry(
            &run_key,
            task_id,
            &task_digest,
            "check-demo__hosted",
            NamedCheckLaneVariant::Hosted,
        ),
        lane_entry(
            &run_key,
            task_id,
            &task_digest,
            "check-demo__local",
            NamedCheckLaneVariant::ScaleSet,
        ),
    ];
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    let plan = Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key).expect("plan ID"),
        base: None,
        head: HEAD.to_owned(),
        event: WorkflowEvent::PullRequest,
        runner: serde_json::from_value::<PlanRunner>(json!({
            "label": "ubuntu-26.04",
            "selection": "latest_default"
        }))
        .expect("plan runner"),
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("no_entry")).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.1".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "cd".repeat(32),
        },
        packages: vec![PlanPackage {
            package_id: "demo 0.1.0".to_owned(),
            name: "demo".to_owned(),
            manifest: "Cargo.toml".to_owned(),
            selected: true,
            reasons: vec!["changed".to_owned()],
            tasks: vec![task_id.to_owned()],
        }],
        obligations: vec![PlanObligation {
            task_id: task_id.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "changed".to_owned(),
            task_digest: task_digest.clone(),
            input_digest: digest_b3(b"inputs"),
            closure_digest: digest_b3(b"closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: entries.to_vec(),
        },
        task_ids: vec![task_id.to_owned()],
        artifact_tasks: vec![],
        warnings: vec![],
        edges: vec![],
    };
    plan.validate().expect("valid plan");
    plan
}

fn lane_entry(
    run_key: &str,
    task_id: &str,
    task_digest: &str,
    job_id: &str,
    variant: NamedCheckLaneVariant,
) -> MatrixEntry {
    MatrixEntry::derive_for_lane(
        "mise",
        task_id,
        "mise check:all",
        task_digest,
        json!({"check_id":"demo"}),
        ExecuteTaskIds::default(),
        &digest_b3(b"entry"),
        run_key,
        job_id,
        Some(variant),
    )
    .expect("typed lane entry")
}

fn make_receipts(plan: &Plan) -> Vec<TaskRuntimeReceipt> {
    plan.matrix
        .include
        .iter()
        .map(|entry| {
            let identity = TaskRuntimeIdentity::new(
                REPOSITORY.to_owned(),
                RUN_ID.to_string(),
                ATTEMPT,
                HEAD.to_owned(),
                format!("{REPOSITORY}/.github/workflows/ci.yml@refs/heads/main"),
                entry.job_id.clone(),
                "runner-name-is-not-used".to_owned(),
            )
            .expect("runtime identity");
            let report_id =
                task_report_id_for_task(&plan.run_key, &entry.matrix_key, &entry.task_digest)
                    .expect("task report ID");
            TaskRuntimeReceipt::derive(plan, entry, &report_id, &identity)
                .expect("plan-bound receipt")
        })
        .collect()
}

fn make_producer_outputs(plan: &Plan, plan_digest: &str) -> TaskReportOutputFanIn {
    let mut keys = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.job_id.as_str())
        .collect::<Vec<_>>();
    keys.sort_unstable();
    let producers = keys
        .iter()
        .map(|job_id| {
            let entry = plan
                .matrix
                .include
                .iter()
                .find(|entry| entry.job_id == *job_id)
                .expect("plan lane");
            let (artifact_id, check_run_id) = ids_for_lane(entry.lane_variant.expect("lane"));
            json!({
                "workflow_job_key": job_id,
                "conclusion": "success",
                "artifact_id": artifact_id,
                "check_run_id": check_run_id,
            })
        })
        .collect::<Vec<_>>();
    TaskReportOutputFanIn::parse_value(json!({
        "schema": 1,
        "origin": "github_com",
        "run": ArtifactBuildRunContext {
            repository_id: REPOSITORY_ID.to_string(),
            repository: REPOSITORY.to_owned(),
            run_id: RUN_ID.to_string(),
            run_attempt: ATTEMPT,
        },
        "head_sha": HEAD,
        "plan_digest": plan_digest,
        "expected_workflow_job_keys": keys,
        "producers": producers,
    }))
    .expect("valid producer fan-in")
}

fn make_provider_evidence(plan: &Plan) -> Value {
    let mut jobs = Vec::new();
    let mut artifacts = Vec::new();
    for entry in &plan.matrix.include {
        let (artifact_id, check_run_id) = ids_for_lane(entry.lane_variant.expect("lane"));
        jobs.push(json!({
            "id": check_run_id + 10_000,
            "check_run_id": check_run_id,
            "run_id": RUN_ID,
            "name": "untrusted provider display name",
            "head_sha": HEAD,
            "status": "completed",
            "conclusion": "success",
            "runner_id": null,
            "runner_name": null,
            "runner_group_id": null,
            "runner_group_name": null,
            "workflow_name": "CI",
            "head_branch": "feature",
            "labels": ["ubuntu-26.04"]
        }));
        artifacts.push(json!({
            "id": artifact_id,
            "name": entry.artifact_id,
            "size_in_bytes": 10,
            "expired": false,
            "digest": null,
            "workflow_run_id": RUN_ID,
            "repository_id": REPOSITORY_ID,
            "head_repository_id": REPOSITORY_ID,
            "head_branch": "feature",
            "head_sha": HEAD
        }));
    }
    json!({
        "repository_id": REPOSITORY_ID,
        "repository_full_name": REPOSITORY,
        "workflow_run_id": RUN_ID,
        "attempt": ATTEMPT,
        "head_sha": plan.head,
        "workflow_path": ".github/workflows/ci.yml",
        "event": "pull_request",
        "head_branch": "feature",
        "head_repository_id": REPOSITORY_ID,
        "head_repository_full_name": REPOSITORY,
        "status": "completed",
        "conclusion": "success",
        "jobs": jobs,
        "artifacts": artifacts
    })
}

fn ids_for_lane(variant: NamedCheckLaneVariant) -> (i64, i64) {
    match variant {
        NamedCheckLaneVariant::Hosted => (501, 601),
        NamedCheckLaneVariant::ScaleSet => (502, 602),
    }
}
