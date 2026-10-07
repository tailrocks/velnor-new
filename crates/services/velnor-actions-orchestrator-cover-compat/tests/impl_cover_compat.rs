//! Compatibility derivation: deterministic, shape-bound digests.
use velnor_actions_contract::validate_digest;
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ObligationDecision, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner,
    Trust, WorkflowEvent,
};
use velnor_actions_orchestrator_cover_compat::cover_compat::{
    baseline_artifact_numeric_id, baseline_compat_for_plan,
};

fn obligation(task_id: &str, task_digest: &str) -> PlanObligation {
    PlanObligation {
        task_id: task_id.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest: task_digest.to_owned(),
        input_digest: task_digest.to_owned(),
        closure_digest: task_digest.to_owned(),
        baseline_proof: None,
    }
}

fn plan_with(label: &str, obligations: Vec<PlanObligation>) -> Plan {
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
        event: WorkflowEvent::Push,
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
        matrix: PlanMatrix { include: vec![] },
        task_ids,
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

#[test]
fn compat_is_deterministic_digest() {
    let plan = plan_with("ubuntu-26.04", vec![obligation("a", &digest(1))]);
    let first = baseline_compat_for_plan(&plan).expect("compat");
    let second = baseline_compat_for_plan(&plan).expect("compat");
    assert_eq!(first, second);
    assert!(validate_digest(&first).is_ok());
}

#[test]
fn compat_ignores_obligation_order() {
    let forward = plan_with(
        "ubuntu-26.04",
        vec![obligation("a", &digest(1)), obligation("b", &digest(2))],
    );
    let backward = plan_with(
        "ubuntu-26.04",
        vec![obligation("b", &digest(2)), obligation("a", &digest(1))],
    );
    assert_eq!(
        baseline_compat_for_plan(&forward).expect("forward"),
        baseline_compat_for_plan(&backward).expect("backward")
    );
}

#[test]
fn compat_follows_runner_label() {
    let left = plan_with("ubuntu-26.04", vec![obligation("a", &digest(1))]);
    let right = plan_with("macos-15", vec![obligation("a", &digest(1))]);
    assert_ne!(
        baseline_compat_for_plan(&left).expect("left"),
        baseline_compat_for_plan(&right).expect("right")
    );
}

#[test]
fn compat_follows_task_digests() {
    let left = plan_with("ubuntu-26.04", vec![obligation("a", &digest(1))]);
    let right = plan_with("ubuntu-26.04", vec![obligation("a", &digest(2))]);
    assert_ne!(
        baseline_compat_for_plan(&left).expect("left"),
        baseline_compat_for_plan(&right).expect("right")
    );
}

#[test]
fn compat_follows_task_set() {
    let one = plan_with("ubuntu-26.04", vec![obligation("a", &digest(1))]);
    let two = plan_with(
        "ubuntu-26.04",
        vec![obligation("a", &digest(1)), obligation("b", &digest(2))],
    );
    assert_ne!(
        baseline_compat_for_plan(&one).expect("one"),
        baseline_compat_for_plan(&two).expect("two")
    );
}

#[test]
fn compat_defined_for_empty_plans() {
    let plan = plan_with("ubuntu-26.04", vec![]);
    let compat = baseline_compat_for_plan(&plan).expect("empty compat");
    assert!(validate_digest(&compat).is_ok());
}

#[test]
fn numeric_id_is_deterministic() {
    let name = "velnor-baseline-abc-123";
    assert_eq!(
        baseline_artifact_numeric_id(name),
        baseline_artifact_numeric_id(name)
    );
}

#[test]
fn numeric_id_is_nonzero_for_any_name() {
    for name in ["", "a", "velnor-baseline-abc-123", &"x".repeat(300)] {
        assert!(baseline_artifact_numeric_id(name) > 0, "{name:?}");
    }
}

#[test]
fn numeric_id_is_name_bound() {
    assert_ne!(
        baseline_artifact_numeric_id("velnor-baseline-aaa"),
        baseline_artifact_numeric_id("velnor-baseline-aab")
    );
}

#[test]
fn numeric_id_matches_first_digest_bytes() {
    let name = "velnor-baseline-abc-123";
    let digest = velnor_actions_contract::digest_b3(name.as_bytes());
    let hex = digest.strip_prefix("b3-").expect("prefix")[..16].to_owned();
    let expected = u64::from_str_radix(&hex, 16).unwrap_or(0).max(1);
    assert_eq!(baseline_artifact_numeric_id(name), expected);
}

#[test]
fn compat_ignores_unbound_plan_fields() {
    let mut left = plan_with("ubuntu-26.04", vec![obligation("a", &digest(1))]);
    let mut right = left.clone();
    left.head = "head-a".to_owned();
    right.head = "head-b".to_owned();
    left.warnings = vec!["w".to_owned()];
    assert_eq!(
        baseline_compat_for_plan(&left).expect("left"),
        baseline_compat_for_plan(&right).expect("right")
    );
}

#[test]
fn compat_ignores_decision_and_reason() {
    let mut plan = plan_with("ubuntu-26.04", vec![obligation("a", &digest(1))]);
    let before = baseline_compat_for_plan(&plan).expect("before");
    plan.obligations[0].decision = ObligationDecision::CoveredByTrustedBaseline;
    plan.obligations[0].reason = "covered".to_owned();
    assert_eq!(baseline_compat_for_plan(&plan).expect("after"), before);
}
