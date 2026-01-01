//! Compatibility derivation tests: shape binding, not content binding.

use super::*;
use velnor_actions_contract::{artifact_id_for_baseline, validate_digest};
use velnor_actions_contract_config::RunnerSelection;
use velnor_actions_contract_workflow::{
    ObligationDecision, PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, Trust,
    WorkflowEvent,
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
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids,
        warnings: Vec::new(),
        edges: Vec::new(),

        artifact_tasks: Vec::new(),
    }
}

mod cover_compat_tests;
