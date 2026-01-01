//! Compatibility derivation tests: shape binding, not content binding.

use super::*;
use velnor_actions_contract::{
    ObligationDecision, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner,
    RunnerSelection, Trust, WorkflowEvent, artifact_id_for_baseline, validate_digest,
};

/// Obligation with explicit digests.
fn obligation(task_id: &str, task_digest: &str, input_digest: &str) -> PlanObligation {
    PlanObligation {
        task_id: task_id.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest: task_digest.to_owned(),
        input_digest: input_digest.to_owned(),
        closure_digest: input_digest.to_owned(),
        baseline_proof: None,
    }
}

/// Plan with `label` and `obligations`.
fn plan_with(label: &str, obligations: Vec<PlanObligation>) -> Plan {
    let task_ids: Vec<String> = obligations
        .iter()
        .map(|obligation| obligation.task_id.clone())
        .collect();
    Plan {
        schema: Plan::SCHEMA,
        run_key: "local".to_owned(),
        plan_id: "plan-local".to_owned(),
        base: None,
        head: "head".to_owned(),
        event: WorkflowEvent::Push,
        qualification: None,
        runner: PlanRunner {
            label: label.to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Trusted,
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
fn compat_is_deterministic_digest() {
    let obligations = vec![
        obligation(
            "stack/rust/a/clippy/default",
            &digest_b3(b"t1"),
            &digest_b3(b"i1"),
        ),
        obligation(
            "stack/rust/b/clippy/default",
            &digest_b3(b"t2"),
            &digest_b3(b"i2"),
        ),
    ];
    let plan = plan_with("ubuntu-26.04", obligations);
    let first = baseline_compat_for_plan(&plan).expect("compat");
    let second = baseline_compat_for_plan(&plan).expect("compat");
    assert_eq!(first, second);
    assert!(validate_digest(&first).is_ok());
}

#[test]
fn compat_ignores_obligation_order() {
    let left = plan_with(
        "ubuntu-26.04",
        vec![
            obligation(
                "stack/rust/a/clippy/default",
                &digest_b3(b"t1"),
                &digest_b3(b"i1"),
            ),
            obligation(
                "stack/rust/b/clippy/default",
                &digest_b3(b"t2"),
                &digest_b3(b"i2"),
            ),
        ],
    );
    let right = plan_with(
        "ubuntu-26.04",
        vec![
            obligation(
                "stack/rust/b/clippy/default",
                &digest_b3(b"t2"),
                &digest_b3(b"i2"),
            ),
            obligation(
                "stack/rust/a/clippy/default",
                &digest_b3(b"t1"),
                &digest_b3(b"i1"),
            ),
        ],
    );
    assert_eq!(
        baseline_compat_for_plan(&left).expect("compat"),
        baseline_compat_for_plan(&right).expect("compat")
    );
}

#[test]
fn compat_survives_source_edits_but_not_toolchain_or_shape() {
    let base = plan_with(
        "ubuntu-26.04",
        vec![obligation(
            "stack/rust/a/clippy/default",
            &digest_b3(b"task"),
            &digest_b3(b"input-v1"),
        )],
    );
    let edited = plan_with(
        "ubuntu-26.04",
        vec![obligation(
            "stack/rust/a/clippy/default",
            &digest_b3(b"task"),
            &digest_b3(b"input-v2"),
        )],
    );
    assert_eq!(
        baseline_compat_for_plan(&base).expect("compat"),
        baseline_compat_for_plan(&edited).expect("compat"),
        "source content never invalidates the execution shape"
    );
    let retooled = plan_with(
        "ubuntu-26.04",
        vec![obligation(
            "stack/rust/a/clippy/default",
            &digest_b3(b"task-v2"),
            &digest_b3(b"input-v1"),
        )],
    );
    assert_ne!(
        baseline_compat_for_plan(&base).expect("compat"),
        baseline_compat_for_plan(&retooled).expect("compat"),
        "task digest drift must invalidate"
    );
    let relabeled = plan_with(
        "ubuntu-24.04",
        vec![obligation(
            "stack/rust/a/clippy/default",
            &digest_b3(b"task"),
            &digest_b3(b"input-v1"),
        )],
    );
    assert_ne!(
        baseline_compat_for_plan(&base).expect("compat"),
        baseline_compat_for_plan(&relabeled).expect("compat"),
        "runner label drift must invalidate"
    );
    let grown = plan_with(
        "ubuntu-26.04",
        vec![
            obligation(
                "stack/rust/a/clippy/default",
                &digest_b3(b"task"),
                &digest_b3(b"input-v1"),
            ),
            obligation(
                "stack/rust/b/clippy/default",
                &digest_b3(b"task"),
                &digest_b3(b"input-v1"),
            ),
        ],
    );
    assert_ne!(
        baseline_compat_for_plan(&base).expect("compat"),
        baseline_compat_for_plan(&grown).expect("compat"),
        "task set drift must invalidate"
    );
}

#[test]
fn compat_defined_for_empty_plans() {
    let plan = plan_with("ubuntu-26.04", Vec::new());
    let compat = baseline_compat_for_plan(&plan).expect("compat");
    assert!(validate_digest(&compat).is_ok());
}

#[test]
fn numeric_id_is_deterministic_nonzero_and_name_bound() {
    let base = "a".repeat(40);
    let compat = digest_b3(b"compat");
    let name = artifact_id_for_baseline(&base, &compat).expect("name");
    let first = baseline_artifact_numeric_id(&name);
    assert_eq!(first, baseline_artifact_numeric_id(&name));
    assert!(first > 0);
    let other = artifact_id_for_baseline(&base, &digest_b3(b"other")).expect("name");
    assert_ne!(
        first,
        baseline_artifact_numeric_id(&other),
        "distinct names must fingerprint distinctly"
    );
}
