use std::collections::BTreeMap;
use std::fs;

use velnor_actions_contract::plan_id_for_run;
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, Trust, WorkflowEvent,
};
use velnor_actions_orchestrator_baseline_publish::baseline_publish::{
    PUBLISH_OP, baseline_publish_to, write_publish_request,
};

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

/// Push publish request JSON over `head`.
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

/// Valid request with one field replaced.
fn request_with(field: &str, value: serde_json::Value) -> String {
    let mut request: serde_json::Value =
        serde_json::from_str(&request_json(&"a".repeat(40))).expect("request parses");
    request[field] = value;
    request.to_string()
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
fn staged_manifest(temp: &std::path::Path, run_key: &str) -> serde_json::Value {
    let bytes = fs::read(temp.join("velnor").join(run_key).join("baseline.json")).expect("staged");
    serde_json::from_slice(&bytes).expect("manifest json")
}

#[test]
fn malformed_request_fails_before_any_gate() {
    let temp = tempfile::tempdir().expect("tempdir");
    assert!(baseline_publish_to("not json", "r7-a1", temp.path()).is_err());
}

#[test]
fn wrong_schema_rejects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let request = request_with("schema", serde_json::json!(2));
    assert!(baseline_publish_to(&request, "r7-a1", temp.path()).is_err());
}

#[test]
fn op_mismatch_rejects_plan_requests() {
    let temp = tempfile::tempdir().expect("tempdir");
    let request = request_with("op", serde_json::json!("plan-v1"));
    let err = baseline_publish_to(&request, "r7-a1", temp.path()).expect_err("op judged");
    assert!(err.to_string().contains("op_mismatch"), "{err}");
}

#[test]
fn malformed_run_key_fails_before_any_gate() {
    let temp = tempfile::tempdir().expect("tempdir");
    assert!(baseline_publish_to(&request_json(&"a".repeat(40)), "bogus", temp.path()).is_err());
}

#[test]
fn local_run_refuses_to_publish() {
    let temp = tempfile::tempdir().expect("tempdir");
    let err = baseline_publish_to(&request_json(&"a".repeat(40)), "local", temp.path())
        .expect_err("local refused");
    assert!(
        err.to_string().contains("publish_refused:local_run"),
        "{err}"
    );
}

#[test]
fn wrong_event_refuses_to_publish() {
    let temp = tempfile::tempdir().expect("tempdir");
    let request = request_with("event", serde_json::json!("pull_request"));
    let err = baseline_publish_to(&request, "r7-a1", temp.path()).expect_err("non-push refused");
    assert!(
        err.to_string().contains("publish_refused:wrong_event"),
        "{err}"
    );
}

#[test]
fn unprotected_ref_refuses_to_publish() {
    let temp = tempfile::tempdir().expect("tempdir");
    let request = request_with("git_ref", serde_json::json!("refs/heads/other"));
    let err = baseline_publish_to(&request, "r7-a1", temp.path()).expect_err("ref refused");
    assert!(
        err.to_string().contains("publish_refused:unprotected_ref"),
        "{err}"
    );
}

#[test]
fn unanchored_repository_refuses_to_publish() {
    let temp = tempfile::tempdir().expect("tempdir");
    let request = request_with("repository", serde_json::Value::Null);
    let err = baseline_publish_to(&request, "r7-a1", temp.path()).expect_err("slug refused");
    assert!(
        err.to_string()
            .contains("publish_refused:repository_unanchored"),
        "{err}"
    );
}

#[test]
fn missing_plan_surfaces_not_found() {
    let temp = tempfile::tempdir().expect("tempdir");
    let err = baseline_publish_to(&request_json(&"a".repeat(40)), "r7-a1", temp.path())
        .expect_err("missing plan errors");
    assert!(err.to_string().contains("not_found"), "{err}");
}

#[test]
fn head_mismatch_refuses_stale_plans() {
    let plan = fixture_plan(&"b".repeat(40), "r7-a1");
    let temp = staged_run(&plan, "r7-a1");
    let err = baseline_publish_to(&request_json(&"a".repeat(40)), "r7-a1", temp.path())
        .expect_err("stale refused");
    assert!(
        err.to_string().contains("publish_refused:head_mismatch"),
        "{err}"
    );
}

#[test]
fn plan_event_mismatch_refuses_non_push_plans() {
    let head = "a".repeat(40);
    let mut plan = fixture_plan(&head, "r7-a1");
    plan.event = WorkflowEvent::PullRequest;
    let temp = staged_run(&plan, "r7-a1");
    let err =
        baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).expect_err("event refused");
    assert!(
        err.to_string()
            .contains("publish_refused:plan_event_mismatch"),
        "{err}"
    );
}

#[test]
fn unproven_reuse_refuses_to_publish() {
    let head = "a".repeat(40);
    let mut plan = fixture_plan(&head, "r7-a1");
    plan.obligations[0].decision = ObligationDecision::ReusedFromTaskCache;
    let temp = staged_run(&plan, "r7-a1");
    let err =
        baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).expect_err("reuse refused");
    assert!(
        err.to_string().contains("publish_refused:unproven_reuse"),
        "{err}"
    );
}

#[test]
fn unverifiable_generator_refuses_to_publish() {
    let head = "a".repeat(40);
    let mut plan = fixture_plan(&head, "r7-a1");
    plan.generator.sha256 = "0".repeat(64);
    let temp = staged_run(&plan, "r7-a1");
    let err = baseline_publish_to(&request_json(&head), "r7-a1", temp.path())
        .expect_err("generator refused");
    assert!(
        err.to_string()
            .contains("publish_refused:generator_unverifiable"),
        "{err}"
    );
}

#[test]
fn malformed_head_refuses_bad_source_commit() {
    let head = "z".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let temp = staged_run(&plan, "r7-a1");
    let err =
        baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).expect_err("head refused");
    assert!(
        err.to_string()
            .contains("publish_refused:bad_source_commit"),
        "{err}"
    );
}

#[test]
fn covered_obligations_skip_without_carrying_proof() {
    use velnor_actions_contract::digest_b3;
    use velnor_actions_contract_workflow::BaselineProof;
    let head = "a".repeat(40);
    let mut plan = fixture_plan(&head, "r7-a1");
    let compat = digest_b3(b"compat");
    let name = format!("velnor-baseline-{head}-{compat}");
    let numeric =
        velnor_actions_orchestrator_cover_compat::cover_compat::baseline_artifact_numeric_id(&name);
    let proof = BaselineProof::new(&head, 7, numeric, &name, &digest_b3(b"manifest"))
        .expect("proof constructs");
    plan.obligations[1].decision = ObligationDecision::CoveredByTrustedBaseline;
    plan.obligations[1].reason = "covered_by_trusted_baseline".to_owned();
    plan.obligations[1].baseline_proof = Some(proof);
    plan.validate().expect("covered fixture validates");
    let temp = staged_run(&plan, "r7-a1");
    baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).expect("publish");
    let staged = staged_manifest(temp.path(), "r7-a1");
    let tasks = staged["tasks"].as_array().expect("tasks array");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["task_id"], "stack/rust/demo/clippy/default");
}

#[test]
fn success_stages_byte_identical_manifests() {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let first = staged_run(&plan, "r7-a1");
    let second = staged_run(&plan, "r7-a1");
    let left = baseline_publish_to(&request_json(&head), "r7-a1", first.path()).expect("publish");
    let right = baseline_publish_to(&request_json(&head), "r7-a1", second.path()).expect("publish");
    assert_eq!(left.artifact_name, right.artifact_name);
    assert!(
        left.artifact_name
            .starts_with(&format!("velnor-baseline-{head}-")),
        "{}",
        left.artifact_name
    );
    let left_bytes = fs::read(
        first
            .path()
            .join("velnor")
            .join("r7-a1")
            .join("baseline.json"),
    )
    .expect("staged");
    let right_bytes = fs::read(
        second
            .path()
            .join("velnor")
            .join("r7-a1")
            .join("baseline.json"),
    )
    .expect("staged");
    assert_eq!(left_bytes, right_bytes);
}

#[test]
fn write_request_materializes_canonical_push_request() {
    let anchor = tempfile::tempdir().expect("tempdir");
    let request_path = anchor
        .path()
        .join("velnor")
        .join("publish-baseline-v1-request.json");
    let head = "a".repeat(40);
    let payload = serde_json::json!({
        "ref": "refs/heads/testmain",
        "before": "b".repeat(40),
        "after": head,
        "repository": {"default_branch": "testmain"},
    })
    .to_string();
    let written = write_publish_request(
        &request_path,
        "push",
        &payload,
        Some(&head),
        Some("o/r"),
        anchor.path(),
    )
    .expect("request written");
    assert_eq!(written, request_path);
    let request: serde_json::Value =
        serde_json::from_slice(&fs::read(&request_path).expect("request file"))
            .expect("request json");
    assert_eq!(request["op"], PUBLISH_OP);
    assert_eq!(request["event"], "push");
    assert_eq!(request["head"], head);
    assert_eq!(request["git_ref"], "refs/heads/testmain");
    assert_eq!(request["default_branch"], "testmain");
}

#[test]
fn write_request_rejects_malformed_payloads() {
    let anchor = tempfile::tempdir().expect("tempdir");
    let request_path = anchor
        .path()
        .join("velnor")
        .join("publish-baseline-v1-request.json");
    let err = write_publish_request(
        &request_path,
        "push",
        "{not json",
        None,
        None,
        anchor.path(),
    )
    .expect_err("payload judged");
    assert!(err.to_string().contains("malformed_event_payload"), "{err}");
}
