use std::collections::BTreeMap;
use velnor_actions_contract::plan_id_for_run;
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, Trust, WorkflowEvent,
};

/// Clippy fixture task ID.
pub(super) const CLIPPY: &str = "stack/rust/demo/clippy/default";

/// Test fixture task ID.
pub(super) const TEST: &str = "stack/rust/demo/test/default";

/// One single-task plan entry plus its obligation digest.
pub(super) fn entry_for(
    stack: &str,
    task_id: &str,
    kind: &str,
    seed: u8,
    job_id: &str,
) -> (MatrixEntry, String) {
    let task_digest = digest(seed);
    let entry = MatrixEntry::derive(
        stack,
        task_id,
        "true",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(kind.to_owned(), ExecuteTaskRef::Single(task_id.to_owned()))]),
        },
        &digest(seed + 10),
        "local",
        job_id,
    )
    .expect("entry derives");
    (entry, task_digest)
}

/// Valid two-task fixture plan (clippy plus test).
pub(super) fn fixture_plan() -> Plan {
    let (clippy_entry, clippy_digest) = entry_for("rust", CLIPPY, "clippy", 1, "crate_clippy");
    let (test_entry, test_digest) = entry_for("rust", TEST, "test", 2, "crate_test");
    let plan = Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan id"),
        base: None,
        head: "HEAD".to_owned(),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![
            obligation_for(CLIPPY, clippy_digest, 1),
            obligation_for(TEST, test_digest, 2),
        ],
        matrix: PlanMatrix {
            include: vec![clippy_entry, test_entry],
        },
        task_ids: vec![CLIPPY.to_owned(), TEST.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),

        artifact_tasks: Vec::new(),
    };
    plan.validate().expect("fixture validates");
    plan
}

/// One execute obligation.
pub(super) fn obligation_for(task_id: &str, task_digest: String, seed: u8) -> PlanObligation {
    PlanObligation {
        task_id: task_id.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest,
        input_digest: digest(seed + 10),
        closure_digest: digest(seed + 20),
        baseline_proof: None,
    }
}

/// One `b3-` digest with every byte set to `byte`.
fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}
