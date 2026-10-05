//! Covered-task encoding and skip-gate tests.

use super::*;
use velnor_actions_contract::{
    PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, RunnerSelection, Trust,
    WorkflowEvent, digest_b3,
};

/// Obligation with one decision.
fn obligation(task_id: &str, decision: ObligationDecision) -> PlanObligation {
    let digest = digest_b3(b"digest");
    PlanObligation {
        task_id: task_id.to_owned(),
        decision,
        reason: "test".to_owned(),
        task_digest: digest.clone(),
        input_digest: digest.clone(),
        closure_digest: digest,
        baseline_proof: None,
    }
}

/// Plan with `obligations`.
fn plan_with(obligations: Vec<PlanObligation>) -> Plan {
    let task_ids: Vec<String> = obligations
        .iter()
        .map(|obligation| obligation.task_id.clone())
        .collect();
    Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: "plan-local".to_owned(),
        base: None,
        head: "head".to_owned(),
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
            sha256: "1".repeat(64),
        },
        packages: Vec::new(),
        obligations,
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids,
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

#[test]
fn encode_wraps_sorted_covered_only() {
    let plan = plan_with(vec![
        obligation("stack/rust/b/clippy/default", ObligationDecision::Execute),
        obligation(
            "stack/rust/z/clippy/default",
            ObligationDecision::CoveredByTrustedBaseline,
        ),
        obligation(
            "stack/rust/a/clippy/default",
            ObligationDecision::CoveredByTrustedBaseline,
        ),
        obligation(
            "stack/rust/q/clippy/default",
            ObligationDecision::ReusedFromTaskCache,
        ),
    ]);
    assert_eq!(
        CoveredTasks::for_plan(&plan).encode(),
        ",stack/rust/a/clippy/default,stack/rust/z/clippy/default,"
    );
}

#[test]
fn encode_empty_matches_nothing() {
    let plan = plan_with(vec![obligation(
        "stack/rust/a/clippy/default",
        ObligationDecision::Execute,
    )]);
    let encoded = CoveredTasks::for_plan(&plan).encode();
    assert!(encoded.is_empty());
    assert!(
        !encoded.contains(",stack/rust/a/clippy/default,"),
        "empty coverage must execute everything"
    );
}

#[test]
fn wrapped_encoding_never_prefix_matches() {
    let encoded = ",stack/rust/demo/clippy/default,";
    assert!(
        !encoded.contains(",stack/rust/demo/clippy,"),
        "a bare kind path must not match its package-qualified sibling"
    );
    assert!(
        !encoded.contains(",stack/rust/demo2/clippy/default,"),
        "a longer package name must not match its prefix"
    );
    assert!(encoded.contains(",stack/rust/demo/clippy/default,"));
}

#[test]
fn covered_by_baseline_matches_decision_exactly() {
    let plan = plan_with(vec![
        obligation("stack/rust/a/clippy/default", ObligationDecision::Execute),
        obligation(
            "stack/rust/b/clippy/default",
            ObligationDecision::CoveredByTrustedBaseline,
        ),
    ]);
    assert!(!covered_by_baseline(&plan, "stack/rust/a/clippy/default"));
    assert!(covered_by_baseline(&plan, "stack/rust/b/clippy/default"));
    assert!(!covered_by_baseline(&plan, "stack/rust/c/clippy/default"));
}

#[test]
fn skip_condition_gates_exact_id() {
    assert_eq!(
        skip_condition("stack/rust/a/clippy/default").expect("condition"),
        "!contains(needs.plan.outputs.covered_tasks, ',stack/rust/a/clippy/default,')"
    );
}

#[test]
fn skip_condition_rejects_malformed_ids() {
    for bad in [
        "",
        "not-a-task",
        "stack/rust/a/clippy/default,stack/rust/b/clippy/default",
        "stack/rust/a/clippy/default'}",
    ] {
        assert!(
            skip_condition(bad).is_err(),
            "malformed IDs must never reach generated expressions: {bad:?}"
        );
    }
}
