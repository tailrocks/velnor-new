//! Publish-baseline tests: staging plus every refusal.
//!
//! Declared via `#[path]` from `baseline_publish.rs` under `cfg(test)`.

use std::collections::BTreeMap;
use std::fs;

use super::*;
use velnor_actions_contract::{artifact_id_for_baseline, plan_id_for_run};
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, Trust,
};

/// Push payload over `head` on the default branch.
fn push_payload(head: &str) -> String {
    serde_json::json!({
        "ref": "refs/heads/testmain",
        "before": "b".repeat(40),
        "after": head,
        "repository": {"default_branch": "testmain"},
    })
    .to_string()
}

/// Publish request JSON over `head` with explicit fields.
fn request_json(head: &str) -> String {
    serde_json::json!({
        "schema": 1,
        "op": PUBLISH_OP,
        "event": "push",
        "head": head,
        "repository": "o/r",
        "git_ref": "refs/heads/testmain",
        "default_branch": "testmain",
    })
    .to_string()
}

/// One `b3-` digest with every byte set to `byte`.
fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

/// One single-task plan entry plus its obligation digest.
fn entry_for(task_id: &str, kind: &str, seed: u8, run_key: &str) -> (MatrixEntry, String) {
    let task_digest = digest(seed);
    let entry = MatrixEntry::derive(
        "rust",
        task_id,
        "true",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(kind.to_owned(), ExecuteTaskRef::Single(task_id.to_owned()))]),
        },
        &digest(seed + 10),
        run_key,
        "rust-demo",
    )
    .expect("entry derives");
    (entry, task_digest)
}

/// One obligation with an explicit decision.
fn obligation_for(
    task_id: &str,
    task_digest: String,
    seed: u8,
    decision: ObligationDecision,
) -> PlanObligation {
    PlanObligation {
        task_id: task_id.to_owned(),
        decision,
        reason: "test".to_owned(),
        task_digest,
        input_digest: digest(seed + 10),
        closure_digest: digest(seed + 20),
        baseline_proof: None,
    }
}

/// Valid push fixture plan over `head` with clippy plus test.
fn fixture_plan(head: &str, run_key: &str) -> Plan {
    let clippy = "stack/rust/demo/clippy/default";
    let test = "stack/rust/demo/test/default";
    let (clippy_entry, clippy_digest) = entry_for(clippy, "clippy", 1, run_key);
    let (test_entry, test_digest) = entry_for(test, "test", 2, run_key);
    let plan = Plan {
        schema: 1,
        run_key: run_key.to_owned(),
        plan_id: plan_id_for_run(run_key).expect("plan id"),
        base: Some("b".repeat(40)),
        head: head.to_owned(),
        event: WorkflowEvent::Push,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Trusted,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "a".repeat(64),
        },
        packages: Vec::new(),
        obligations: vec![
            obligation_for(clippy, clippy_digest, 1, ObligationDecision::Execute),
            obligation_for(test, test_digest, 2, ObligationDecision::Execute),
        ],
        matrix: PlanMatrix {
            include: vec![clippy_entry, test_entry],
        },
        task_ids: vec![clippy.to_owned(), test.to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    };
    plan.validate().expect("fixture validates");
    plan
}

/// Staged run dir carrying `plan.json` for `run_key`.
fn staged_run(plan: &Plan, run_key: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().join("velnor").join(run_key);
    fs::create_dir_all(&dir).expect("run dir");
    fs::write(
        dir.join("plan.json"),
        serde_json::to_string(plan).expect("plan json"),
    )
    .expect("plan file");
    temp
}

/// Staged manifest value for one publish run.
fn staged_manifest(temp: &Path, run_key: &str) -> serde_json::Value {
    let bytes =
        fs::read(temp.join("velnor").join(run_key).join(BASELINE_FILENAME)).expect("staged");
    serde_json::from_slice(&bytes).expect("manifest json")
}

mod baseline_publish_tests;
