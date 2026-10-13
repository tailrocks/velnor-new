//! Publish-baseline tests: staging plus every refusal.
//!
//! Declared via `#[path]` from `baseline_publish.rs` under `cfg(test)`.

use std::collections::BTreeMap;
use std::fs;

use super::*;
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, PlannedPlatform, RunnerSelection, Trust,
    artifact_id_for_baseline, plan_id_for_run,
};

#[path = "baseline_publish_request_tests.rs"]
mod request_tests;

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
pub(super) fn request_json(head: &str) -> String {
    request_json_with_base(head, &"b".repeat(40))
}

/// Publish request with an explicit push-before commit.
pub(super) fn request_json_with_base(head: &str, base: &str) -> String {
    serde_json::json!({
        "schema": 1,
        "op": PUBLISH_OP,
        "event": "push",
        "base": base,
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
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("planned platform"),
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
pub(super) fn fixture_plan(head: &str, run_key: &str) -> Plan {
    let clippy = "stack/rust/demo/clippy/default";
    let test = "stack/rust/demo/test/default";
    let (clippy_entry, clippy_digest) = entry_for(clippy, "clippy", 1, run_key);
    let (test_entry, test_digest) = entry_for(test, "test", 2, run_key);
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
    let plan_json = serde_json::to_string(plan).expect("plan json");
    fs::write(dir.join("plan.json"), plan_json).expect("plan file");
    temp
}

/// Staged manifest value for one publish run.
fn staged_manifest(temp: &Path, run_key: &str) -> serde_json::Value {
    let bytes = fs::read(
        temp.join("velnor")
            .join(run_key)
            .join(PUBLISHED_BASELINE_DIR)
            .join(BASELINE_FILENAME),
    )
    .expect("staged");
    serde_json::from_slice(&bytes).expect("manifest json")
}

// Direct-proof staging plus carry lineage cases live apart for the size gate.
#[path = "baseline_publish_lineage_tests.rs"]
mod baseline_publish_lineage_tests;

/// Refusal problem for one request/plan pair.
///
/// Refusals stage nothing: the run directory carries no baseline file
/// after the refused call.
pub(super) fn refuse_problem(request: &str, plan: &Plan, run_key: &str) -> String {
    let temp = staged_run(plan, run_key);
    let problem = baseline_publish_to(request, run_key, temp.path())
        .expect_err("must refuse")
        .to_string();
    assert!(
        !temp
            .path()
            .join("velnor")
            .join(run_key)
            .join(PUBLISHED_BASELINE_DIR)
            .join(BASELINE_FILENAME)
            .exists(),
        "refusals stage nothing: {problem}"
    );
    problem
}

#[test]
fn publish_refuses_every_unsafe_case() {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let request = |patch: serde_json::Value| {
        let mut base: serde_json::Value =
            serde_json::from_str(&request_json(&head)).expect("request");
        for (key, value) in patch.as_object().expect("patch") {
            base[key] = value.clone();
        }
        base.to_string()
    };
    let cases = [
        (
            request(serde_json::json!({"event": "pull_request"})),
            "wrong_event",
        ),
        (
            request(serde_json::json!({"git_ref": "refs/heads/other"})),
            "unprotected_ref",
        ),
        (
            request(serde_json::json!({"default_branch": "other"})),
            "unprotected_ref",
        ),
        (
            request(serde_json::json!({"default_branch": serde_json::Value::Null})),
            "unprotected_ref",
        ),
        (
            request(serde_json::json!({"repository": serde_json::Value::Null})),
            "repository_unanchored",
        ),
        (
            request(serde_json::json!({"repository": "not-a-slug"})),
            "repository_unanchored",
        ),
        (
            request(serde_json::json!({"head": "b".repeat(40)})),
            "head_mismatch",
        ),
        (
            request(serde_json::json!({"base": "c".repeat(40)})),
            "base_mismatch",
        ),
        (request(serde_json::json!({"op": "plan-v1"})), "op_mismatch"),
        (
            request(serde_json::json!({"schema": 2})),
            "unsupported_schema",
        ),
        ("not json".to_owned(), "malformed_json"),
    ];
    for (mutated, reason) in cases {
        let problem = refuse_problem(&mutated, &plan, "r7-a1");
        assert!(problem.contains(reason), "{reason}: {problem}");
    }
    let mut pr_plan = fixture_plan(&head, "r7-a1");
    pr_plan.event = WorkflowEvent::PullRequest;
    pr_plan.trust = Trust::Pr;
    assert!(
        refuse_problem(&request_json(&head), &pr_plan, "r7-a1").contains("plan_event_mismatch")
    );
    let mut zeroed = fixture_plan(&head, "r7-a1");
    zeroed.generator.sha256 = "0".repeat(64);
    assert!(
        refuse_problem(&request_json(&head), &zeroed, "r7-a1").contains("generator_unverifiable")
    );
    let mut reused = fixture_plan(&head, "r7-a1");
    reused.obligations[0].decision = ObligationDecision::ReusedFromTaskCache;
    assert!(refuse_problem(&request_json(&head), &reused, "r7-a1").contains("unproven_reuse"));
    assert!(
        refuse_problem(&request_json(&head), &plan, "local").contains("local_run"),
        "local runs never publish"
    );
}

#[test]
fn publish_rejects_unknown_request_fields() {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let mut request: serde_json::Value =
        serde_json::from_str(&request_json(&head)).expect("request");
    request["generator"] = serde_json::json!("forged");
    assert!(refuse_problem(&request.to_string(), &plan, "r7-a1").contains("malformed_request"));
}

#[test]
fn publish_never_overwrites_staged_evidence() {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let temp = staged_run(&plan, "r7-a1");
    baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).expect("first publish");
    assert!(
        baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).is_err(),
        "a second publish must fail instead of overwriting"
    );
}

#[test]
fn staged_manifest_passes_consumer_validation() {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let temp = staged_run(&plan, "r7-a1");
    baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).expect("publish");
    let staged = staged_manifest(temp.path(), "r7-a1");
    let manifest: BaselineManifest = serde_json::from_value(staged).expect("manifest");
    let expected = ProvenanceExpectations {
        base: head,
        branch: "testmain".to_owned(),
        workflow_path: velnor_actions_workflow_renderer::render::WORKFLOW_PATH.to_owned(),
        generator_version: plan.generator.version.clone(),
        generator_sha256: plan.generator.sha256.clone(),
        repository_id: Some(digest_b3(b"github.com/o/r")),
        repository_slug: Some("o/r".to_owned()),
        repository_conflict: false,
    };
    let bytes = canonical_json_bytes(&manifest).expect("canonical");
    validate_provenance(&manifest, &digest_b3(&bytes), &expected).expect("self-check agrees");
}

#[test]
fn publish_request_writer_records_push_refs() {
    let head = "a".repeat(40);
    let temp = tempfile::tempdir().expect("tempdir");
    let anchor = temp.path().join("anchor");
    fs::create_dir_all(&anchor).expect("anchor");
    let path = anchor.join(format!("{PUBLISH_OP}-request.json"));
    write_publish_request(
        &path,
        "push",
        &push_payload(&head),
        Some(&head),
        Some("o/r"),
        &anchor,
    )
    .expect("write");
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
    assert_eq!(written["schema"], 1);
    assert_eq!(written["op"], PUBLISH_OP);
    assert_eq!(written["event"], "push");
    assert_eq!(written["base"], "b".repeat(40));
    assert_eq!(written["head"], head);
    assert_eq!(written["repository"], "o/r");
    assert_eq!(written["git_ref"], "refs/heads/testmain");
    assert_eq!(written["default_branch"], "testmain");
    let again = write_publish_request(
        &path,
        "push",
        &push_payload(&head),
        Some(&head),
        Some("o/r"),
        &anchor,
    );
    assert!(again.is_err(), "pre-existing requests never overwrite");
    let bad_payload = anchor.join("bad-request.json");
    assert!(
        write_publish_request(
            &bad_payload,
            "push",
            "nope",
            Some(&head),
            Some("o/r"),
            &anchor
        )
        .expect_err("malformed")
        .to_string()
        .contains("malformed_event_payload")
    );
    let bad_event = anchor.join("bad-event.json");
    assert!(
        write_publish_request(
            &bad_event,
            "issue_comment",
            &push_payload(&head),
            Some(&head),
            Some("o/r"),
            &anchor,
        )
        .expect_err("event")
        .to_string()
        .contains("unsupported_event")
    );
}
