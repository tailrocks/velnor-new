//! Merge verification-scope gate cases.

use std::collections::BTreeSet;

use serde_json::json;
use velnor_actions_contract::{
    JobConclusion, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanRunner, RunnerSelection,
    Trust, VerificationScope, WorkflowEvent,
};

use super::super::{MergeRequest, required_evidence};
use super::{check_scope_coherence, runner_scope_matches_event};
use crate::cover::Signals;

/// Minimal plan shape for the pure scope gate.
fn plan(event: WorkflowEvent, scope: VerificationScope) -> Plan {
    Plan {
        producers: Default::default(),
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: "plan-local".to_owned(),
        base: None,
        head: "head".to_owned(),
        event,
        scope,
        runner: PlanRunner {
            label: "ubuntu-24.04".to_owned(),
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
        obligations: Vec::new(),
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids: Vec::new(),
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

/// Decode the merge envelope shape used by assembly, with only the scope
/// fields varied per case.
fn request(
    event: Option<WorkflowEvent>,
    scope: Option<VerificationScope>,
    required_jobs: &[(&str, JobConclusion)],
) -> MergeRequest {
    let jobs: Vec<_> = required_jobs
        .iter()
        .map(|(job_id, conclusion)| json!({"job_id": job_id, "conclusion": conclusion}))
        .collect();
    serde_json::from_value(json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": event,
        "actual_scope": scope,
        "matrix_reports": [],
        "required_job_ids": ["plan"],
        "required_jobs": jobs,
    }))
    .expect("merge request fixture")
}

fn scope_result(plan: &Plan, request: &MergeRequest) -> (Signals, BTreeSet<String>) {
    let mut signals = Signals::default();
    let mut miss_reasons = BTreeSet::new();
    check_scope_coherence(plan, request, &mut signals, &mut miss_reasons);
    (signals, miss_reasons)
}

#[test]
fn runner_full_scope_rejects_affected_plan() {
    let plan = plan(WorkflowEvent::WorkflowDispatch, VerificationScope::Affected);
    let request = request(
        Some(WorkflowEvent::WorkflowDispatch),
        Some(VerificationScope::Full),
        &[("plan", JobConclusion::Success)],
    );
    let (signals, miss_reasons) = scope_result(&plan, &request);
    assert!(signals.planning_failed);
    assert!(miss_reasons.contains("trust_scope_mismatch"));
}

#[test]
fn full_plan_rejects_affected_runner_scope() {
    let plan = plan(WorkflowEvent::Push, VerificationScope::Full);
    let request = request(
        Some(WorkflowEvent::Push),
        Some(VerificationScope::Affected),
        &[("plan", JobConclusion::Success)],
    );
    let (signals, miss_reasons) = scope_result(&plan, &request);
    assert!(signals.planning_failed);
    assert!(miss_reasons.contains("trust_scope_mismatch"));
}

#[test]
fn missing_actual_scope_fails_closed() {
    let plan = plan(WorkflowEvent::Push, VerificationScope::Affected);
    let request = request(
        Some(WorkflowEvent::Push),
        None,
        &[("plan", JobConclusion::Success)],
    );
    let (signals, miss_reasons) = scope_result(&plan, &request);
    assert!(signals.planning_failed);
    assert!(miss_reasons.contains("trust_scope_mismatch"));
}

#[test]
fn schedule_cannot_claim_affected_scope() {
    let plan = plan(WorkflowEvent::Schedule, VerificationScope::Full);
    let request = request(
        Some(WorkflowEvent::Schedule),
        Some(VerificationScope::Affected),
        &[("plan", JobConclusion::Success)],
    );
    let (signals, miss_reasons) = scope_result(&plan, &request);
    assert!(signals.planning_failed);
    assert!(miss_reasons.contains("trust_scope_mismatch"));
}

#[test]
fn schedule_full_scope_passes_gate_but_missing_required_job_fails() {
    let plan = plan(WorkflowEvent::Schedule, VerificationScope::Full);
    let request_value = request(
        Some(WorkflowEvent::Schedule),
        Some(VerificationScope::Full),
        &[],
    );
    let (signals, miss_reasons) = scope_result(&plan, &request_value);
    assert!(!signals.planning_failed, "scope: {miss_reasons:?}");
    assert!(miss_reasons.is_empty());

    let mut signals = Signals::default();
    let mut miss_reasons = BTreeSet::new();
    required_evidence::check_required_evidence(
        &plan,
        &request_value,
        &mut signals,
        &mut miss_reasons,
    );
    assert!(signals.planning_failed);
    assert!(miss_reasons.contains("no_entry"));

    let failed = request(
        Some(WorkflowEvent::Schedule),
        Some(VerificationScope::Full),
        &[("plan", JobConclusion::Failure)],
    );
    let mut signals = Signals::default();
    required_evidence::fold_jobs(&plan, &failed, &mut signals);
    assert!(signals.failed);
}

#[test]
fn canonical_event_scope_is_affected_except_schedule() {
    for event in [
        WorkflowEvent::PullRequest,
        WorkflowEvent::Fork,
        WorkflowEvent::Push,
        WorkflowEvent::MergeGroup,
        WorkflowEvent::Local,
    ] {
        assert!(runner_scope_matches_event(
            event,
            VerificationScope::Affected
        ));
    }
    assert!(runner_scope_matches_event(
        WorkflowEvent::Schedule,
        VerificationScope::Full
    ));
}
