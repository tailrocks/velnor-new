//! Pure runner-authority comparisons; no process environment mutations.

use super::*;
use velnor_actions_contract::{
    PlanBaseline, PlanGenerator, PlanMatrix, PlanRunner, RunnerSelection, Trust, plan_id_for_run,
};

fn fixture() -> (CurrentSourceContext, Plan) {
    let run_key = "r1-a1";
    let head = "a".repeat(40);
    let base = Some("b".repeat(40));
    let context = CurrentSourceContext {
        root: PathBuf::from("/checkout"),
        event_path: PathBuf::from("/runner/event.json"),
        payload_digest: digest_b3(b"{}"),
        event: WorkflowEvent::Push,
        scope: VerificationScope::Affected,
        base: base.clone(),
        event_head: head.clone(),
        candidate: head.clone(),
        github_sha: head.clone(),
        run_key: run_key.to_owned(),
        repository: "owner/repo".to_owned(),
    };
    let plan = Plan {
        schema: 1,
        run_key: run_key.to_owned(),
        plan_id: plan_id_for_run(run_key).expect("plan id"),
        base,
        head,
        event: WorkflowEvent::Push,
        scope: VerificationScope::Affected,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Trusted,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "c".repeat(64),
        },
        packages: Vec::new(),
        obligations: Vec::new(),
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids: Vec::new(),
        warnings: Vec::new(),
        edges: Vec::new(),
        producers: Default::default(),
    };
    (context, plan)
}

#[test]
fn claimed_plan_must_match_each_actual_identity_dimension() {
    let mutations: [(fn(&mut Plan), &str); 5] = [
        (
            |plan| plan.event = WorkflowEvent::Local,
            "current_source_event_mismatch",
        ),
        (
            |plan| plan.scope = VerificationScope::Full,
            "current_source_scope_mismatch",
        ),
        (|plan| plan.base = None, "current_source_base_mismatch"),
        (
            |plan| plan.head = "d".repeat(40),
            "current_source_head_mismatch",
        ),
        (
            |plan| plan.run_key = "r2-a1".to_owned(),
            "current_source_run_mismatch",
        ),
    ];
    let (context, plan) = fixture();
    context.verify_plan(&plan).expect("matching authority");
    assert_eq!(context.run_key(), "r1-a1");
    for (mutate, problem) in mutations {
        let mut claimed = plan.clone();
        mutate(&mut claimed);
        let error = context.verify_plan(&claimed).expect_err("forgery refused");
        assert_eq!(error.to_string(), format!("internal: {problem}"));
    }
}

#[test]
fn actual_local_event_refused_even_with_push_claim() {
    let (_, claimed) = fixture();
    assert_eq!(claimed.event, WorkflowEvent::Push);
    let error = actual_event_for("local", &serde_json::json!({}))
        .expect_err("actual local event cannot acquire source authority");
    assert_eq!(
        error.to_string(),
        "internal: current_source_requires_runner_event"
    );
}

#[test]
fn plan_head_binds_tested_pr_merge_candidate() {
    let (mut context, mut plan) = fixture();
    context.event = WorkflowEvent::PullRequest;
    context.candidate = "d".repeat(40);
    plan.event = WorkflowEvent::PullRequest;
    plan.head = context.candidate.clone();
    context.verify_plan(&plan).expect("tested merge candidate");
    plan.head = context.event_head.clone();
    assert!(
        context.verify_plan(&plan).is_err(),
        "feature head is a different candidate"
    );
}

#[test]
fn context_equality_includes_payload_and_repository_authority() {
    let (mut actual, _) = fixture();
    let (captured, _) = fixture();
    actual.payload_digest = digest_b3(b"{\"changed\":true}");
    assert_ne!(actual, captured);
    actual.payload_digest.clone_from(&captured.payload_digest);
    actual.repository = "other/repo".to_owned();
    assert_ne!(actual, captured);
}

#[test]
fn actual_runner_sha_required_and_strict() {
    for sha in [None, Some(""), Some(" "), Some("HEAD"), Some("abc")] {
        assert!(validate_actual_sha(sha).is_err(), "invalid SHA: {sha:?}");
    }
    validate_actual_sha(Some(&"a".repeat(40))).expect("exact runner SHA");
}

#[test]
fn replacement_merge_candidate_cannot_reuse_actual_pr_parents() {
    let (mut context, _) = fixture();
    context.event = WorkflowEvent::PullRequest;
    context.candidate = "d".repeat(40);
    let error = verify_candidate_sha(&context.github_sha, &context.candidate)
        .expect_err("different merge commit refused despite matching parents");
    assert_eq!(
        error.to_string(),
        "internal: current_source_candidate_sha_mismatch"
    );
    verify_candidate_sha(&context.github_sha, &context.github_sha).expect("exact actual candidate");
}

#[test]
fn payload_repository_requires_matching_runner_authority() {
    for payload in [
        serde_json::json!({}),
        serde_json::json!({"repository": {}}),
        serde_json::json!({"repository": {"full_name": null}}),
        serde_json::json!({"repository": {"full_name": ""}}),
        serde_json::json!({"repository": {"full_name": "owner/repo "}}),
        serde_json::json!({"repository": {"full_name": "other/repo"}}),
    ] {
        assert!(
            verify_payload_repository(&payload, "owner/repo").is_err(),
            "invalid repository channel: {payload}"
        );
    }
    let valid = serde_json::json!({"repository": {"full_name": "Owner/Repo"}});
    verify_payload_repository(&valid, "owner/repo").expect("normalized same repository");
}
