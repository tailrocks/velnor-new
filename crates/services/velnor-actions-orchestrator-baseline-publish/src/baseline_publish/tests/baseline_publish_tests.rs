use super::*;

#[test]
fn publish_stages_trusted_manifest_under_derived_name() {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let temp = staged_run(&plan, "r7-a1");
    let outputs = baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).expect("publish");
    let compat =
        velnor_actions_orchestrator_cover_compat::cover_compat::baseline_compat_for_plan(&plan)
            .expect("compat");
    let expected = artifact_id_for_baseline(&head, &compat).expect("name");
    assert_eq!(outputs.artifact_name, expected);
    let staged = staged_manifest(temp.path(), "r7-a1");
    assert_eq!(staged["schema"], 2);
    assert_eq!(staged["source_commit"], head);
    assert_eq!(staged["ref"], "refs/heads/testmain");
    assert_eq!(staged["event"], "push");
    assert_eq!(
        staged["workflow_ref"],
        "o/r/.github/workflows/ci.yml@refs/heads/testmain"
    );
    assert_eq!(staged["run_id"], 7);
    assert_eq!(staged["run_attempt"], 1);
    assert_eq!(staged["final_status"], "passed");
    assert_eq!(staged["compatibility_id"], compat);
    assert_eq!(staged["artifact_name"], expected);
    assert_eq!(
        staged["artifact_id"],
        velnor_actions_orchestrator_cover_compat::cover_compat::baseline_artifact_numeric_id(
            &expected
        )
    );
    assert_eq!(staged["tasks"].as_array().expect("tasks").len(), 2);
}

#[test]
fn publish_skips_covered_without_carry_forward() {
    let head = "a".repeat(40);
    let mut plan = fixture_plan(&head, "r7-a1");
    let compat = digest(8);
    let name = format!("velnor-baseline-{head}-{compat}");
    let proof = velnor_actions_contract_workflow::BaselineProof::new(
        &head,
        7,
        velnor_actions_orchestrator_cover_compat::cover_compat::baseline_artifact_numeric_id(&name),
        &name,
        &digest(9),
    )
    .expect("proof constructs");
    plan.obligations[1].decision = ObligationDecision::CoveredByTrustedBaseline;
    plan.obligations[1].baseline_proof = Some(proof);
    plan.matrix.include.pop();
    plan.validate().expect("covered plan validates");
    let temp = staged_run(&plan, "r7-a1");
    baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).expect("publish");
    let staged = staged_manifest(temp.path(), "r7-a1");
    let tasks = staged["tasks"].as_array().expect("tasks");
    assert_eq!(tasks.len(), 1, "covered tasks never carry forward");
    assert_eq!(tasks[0]["task_id"], "stack/rust/demo/clippy/default");
    assert_eq!(tasks[0]["proof_run_id"], 7);
    assert_eq!(tasks[0]["observed_run_id"], 7);
}

/// Refusal problem for one request/plan pair.
///
/// Refusals stage nothing: the run directory carries no baseline file
/// after the refused call.
fn refuse_problem(request: &str, plan: &Plan, run_key: &str) -> String {
    let temp = staged_run(plan, run_key);
    let problem = baseline_publish_to(request, run_key, temp.path())
        .expect_err("must refuse")
        .to_string();
    assert!(
        !temp
            .path()
            .join("velnor")
            .join(run_key)
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
