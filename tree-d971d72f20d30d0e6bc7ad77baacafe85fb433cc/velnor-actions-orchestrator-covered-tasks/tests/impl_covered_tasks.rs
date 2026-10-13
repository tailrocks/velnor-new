//! Covered-task collection, encoding, and per-task queries.

use velnor_actions_contract::digest_b3;
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ObligationDecision, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner,
    Trust, WorkflowEvent,
};
use velnor_actions_orchestrator_covered_tasks::covered_tasks::{
    COVERED_TASKS_OUTPUT, CoveredTasks, covered_by_baseline, decision_is_covered, plan_has_covered,
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

        artifact_tasks: Vec::new(),
    }
}

#[test]
fn decision_table_routes_only_baseline_covered() {
    assert!(decision_is_covered(
        ObligationDecision::CoveredByTrustedBaseline
    ));
    assert!(!decision_is_covered(ObligationDecision::Execute));
    assert!(!decision_is_covered(
        ObligationDecision::ReusedFromTaskCache
    ));
}

#[test]
fn plan_without_obligations_has_no_covered() {
    let plan = plan_with(Vec::new());
    assert!(!plan_has_covered(&plan));
    assert_eq!(CoveredTasks::for_plan(&plan).encode(), "");
    assert!(!covered_by_baseline(&plan, "a"));
}

#[test]
fn execute_and_reused_decisions_excluded() {
    let plan = plan_with(vec![
        obligation("a", ObligationDecision::Execute),
        obligation("b", ObligationDecision::ReusedFromTaskCache),
    ]);
    assert!(!plan_has_covered(&plan));
    assert_eq!(CoveredTasks::for_plan(&plan).encode(), "");
}

#[test]
fn covered_ids_sorted_unique_wrapped() {
    let plan = plan_with(vec![
        obligation("b", ObligationDecision::CoveredByTrustedBaseline),
        obligation("a", ObligationDecision::CoveredByTrustedBaseline),
        obligation("a", ObligationDecision::CoveredByTrustedBaseline),
        obligation("c", ObligationDecision::Execute),
    ]);
    assert!(plan_has_covered(&plan));
    assert_eq!(CoveredTasks::for_plan(&plan).encode(), ",a,b,");
}

#[test]
fn single_covered_id_wrapped() {
    let plan = plan_with(vec![obligation(
        "x",
        ObligationDecision::CoveredByTrustedBaseline,
    )]);
    assert_eq!(CoveredTasks::for_plan(&plan).encode(), ",x,");
}

#[test]
fn covered_by_baseline_matches_task_only() {
    let plan = plan_with(vec![
        obligation("a", ObligationDecision::CoveredByTrustedBaseline),
        obligation("b", ObligationDecision::Execute),
    ]);
    assert!(covered_by_baseline(&plan, "a"));
    assert!(!covered_by_baseline(&plan, "b"));
    assert!(!covered_by_baseline(&plan, "zzz"));
}

#[test]
fn output_name_single_sourced() {
    assert_eq!(COVERED_TASKS_OUTPUT, "covered_tasks");
}

#[test]
fn default_tasks_encode_empty() {
    assert_eq!(CoveredTasks::default().encode(), "");
}

#[test]
fn prefix_ids_stay_whole() {
    let plan = plan_with(vec![
        obligation("a", ObligationDecision::CoveredByTrustedBaseline),
        obligation("ab", ObligationDecision::CoveredByTrustedBaseline),
    ]);
    assert_eq!(CoveredTasks::for_plan(&plan).encode(), ",a,ab,");
}

#[test]
fn mixed_plan_reports_covered() {
    let plan = plan_with(vec![
        obligation("a", ObligationDecision::Execute),
        obligation("b", ObligationDecision::CoveredByTrustedBaseline),
    ]);
    assert!(plan_has_covered(&plan));
    assert!(covered_by_baseline(&plan, "b"));
}
