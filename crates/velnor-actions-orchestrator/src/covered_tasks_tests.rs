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
        job_id: "rust-demo".to_owned(),
        decision,
        reason: "test".to_owned(),
        task_digest: digest.clone(),
        input_digest: digest.clone(),
        execution_identity: velnor_actions_contract::TaskExecutionIdentity::new(
            &velnor_actions_contract::digest_b3(b"fixture-graph"),
            &velnor_actions_contract::digest_b3(b"fixture-toolchain"),
            &velnor_actions_contract::digest_b3(b"fixture-mbx"),
            &velnor_actions_contract::digest_b3(b"fixture-platform"),
            "default",
        )
        .expect("execution identity"),
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
        producers: Default::default(),
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: "plan-local".to_owned(),
        base: None,
        head: "head".to_owned(),
        event: WorkflowEvent::PullRequest,
        scope: velnor_actions_contract::VerificationScope::Affected,
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

#[test]
fn job_selection_requires_any_uncovered_obligation_before_allocation() {
    let task = |task_id: &str| CrateObligation {
        task_id: task_id.to_owned(),
        kind: "clippy".to_owned(),
        step_name: "Clippy".to_owned(),
        gated_by: Vec::new(),
        matrix_key: "clippy".to_owned(),
        task_digest: digest_b3(b"task"),
        run: vec!["true".to_owned()],
    };
    let condition = job_condition(&[
        task("stack/rust/a/clippy/default"),
        task("stack/rust/a/test/default"),
    ])
    .expect("condition");
    assert_eq!(
        condition,
        "!cancelled() && needs.plan.result == 'success' && (!contains(needs.plan.outputs.covered_tasks, ',stack/rust/a/clippy/default,') || !contains(needs.plan.outputs.covered_tasks, ',stack/rust/a/test/default,'))"
    );
    assert!(job_condition(&[]).is_err(), "empty lane must fail closed");
}

#[test]
fn empty_partial_complete_and_reuse_outputs_select_soundly() {
    let ids = ["stack/rust/a/clippy/default", "stack/rust/a/test/default"];
    let allocates = |encoded: &str| ids.iter().any(|id| !encoded.contains(&format!(",{id},")));
    assert!(allocates(""), "missing output executes");
    assert!(
        allocates(",stack/rust/a/clippy/default,"),
        "partial coverage executes"
    );
    assert!(!allocates(
        ",stack/rust/a/clippy/default,stack/rust/a/test/default,"
    ));
    let reused = plan_with(
        ids.iter()
            .map(|id| obligation(id, ObligationDecision::ReusedFromTaskCache))
            .collect(),
    );
    assert!(
        allocates(&CoveredTasks::for_plan(&reused).encode()),
        "unqualified task reuse cannot omit a runner"
    );
}
