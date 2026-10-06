use super::*;
use crate::cover::Signals;
use crate::merge::{MergeRequest, required_evidence};
use std::collections::BTreeSet;
use velnor_actions_contract::{
    ObligationDecision, PlanObligation, ProducerEventContext, WorkflowEvent,
};

fn admission() -> ProducerAdmission {
    serde_json::from_value(serde_json::json!({
        "job_id":"tools-example", "policy":"advisory_fallback", "role":{
            "kind":"tool", "producer":{
                "descriptor":{"domain":"planning","target":"x86_64-unknown-linux-gnu",
                    "runs_on":"ubuntu-26.04","selectors":["rust@1.98.0"],
                    "immutable_identity":"tool-qualified-key",
                    "qualification_identity":format!("qualified-tools@b3-{}", "a".repeat(64))},
                "selection":{"tasks":[],"cargo_fallback":false,"unconditional":false},
                "restore_step":"restore","before_step":"before","installation_step":"install",
                "after_step":"after","save_step":"save","report_step":"report"
            }
        }
    }))
    .expect("typed producer")
}

fn plan() -> Plan {
    serde_json::from_value(serde_json::json!({
        "schema":1,"run_key":"run","plan_id":"plan-run","base":null,"head":"a".repeat(40),
        "event":"pull_request","trust":"pr","baseline":{"status":"unavailable"},
        "runner":{"label":"ubuntu-26.04","selection":"latest_default"},
        "generator":{"version":"1.0.0","target":"x86_64-unknown-linux-gnu","sha256":"a".repeat(64)},
        "packages":[],"obligations":[],"matrix":{"include":[]},"task_ids":[],
        "producers":{"context":{"reference":"refs/pull/1/merge","default_branch":"main",
            "protected":false,"cargo_fallback":false},"entries":[admission()]}
    }))
    .expect("plan shape")
}

fn request(conclusion: &str) -> MergeRequest {
    serde_json::from_value(serde_json::json!({
        "schema":1,"run_key":"run","required_job_ids":["plan","tools-example"],
        "required_jobs":[{"job_id":"plan","conclusion":"success"},
            {"job_id":"tools-example","conclusion":conclusion}],
        "matrix_reports":[],"task_reports":[]
    }))
    .expect("request shape")
}

fn terminal() -> ProducerReport {
    ProducerReport {
        job_id: "tools-example".to_owned(),
        identity: "tool-qualified-key".to_owned(),
        verified: false,
        cache_available: false,
        error: ProducerTerminalError::PreparationFailed,
    }
}

#[test]
fn intentional_skip_requires_exact_admission_and_ineligible_context() {
    let mut plan = plan();
    for event in [
        WorkflowEvent::PullRequest,
        WorkflowEvent::Fork,
        WorkflowEvent::MergeGroup,
    ] {
        plan.event = event;
        let mut signals = Signals::default();
        required_evidence::fold_jobs(&plan, &request("skipped"), &mut signals);
        assert!(!signals.not_run, "{event:?}");
    }
    plan.event = WorkflowEvent::Push;
    plan.producers.context = Some(ProducerEventContext {
        reference: "refs/heads/main".to_owned(),
        default_branch: "main".to_owned(),
        protected: true,
        cargo_fallback: false,
    });
    let mut signals = Signals::default();
    required_evidence::fold_jobs(&plan, &request("skipped"), &mut signals);
    assert!(signals.not_run);
    plan.producers.entries.clear();
    let mut signals = Signals::default();
    required_evidence::fold_jobs(&plan, &request("skipped"), &mut signals);
    assert!(signals.not_run);
}

#[test]
fn advisory_failure_requires_exact_terminal_and_explicit_policy() {
    let mut admission = admission();
    let mut report = terminal();
    assert!(advisory_failure(
        &admission,
        JobConclusion::Failure,
        Some(&report)
    ));
    assert!(!advisory_failure(&admission, JobConclusion::Failure, None));
    report.identity = "forged".to_owned();
    assert!(!advisory_failure(
        &admission,
        JobConclusion::Failure,
        Some(&report)
    ));
    report = terminal();
    admission.policy = ProducerPolicy::Mandatory;
    assert!(!advisory_failure(
        &admission,
        JobConclusion::Failure,
        Some(&report)
    ));
    assert!(!advisory_failure(
        &admission,
        JobConclusion::Cancelled,
        Some(&report)
    ));
}

#[test]
fn missing_forged_and_unexpected_terminal_evidence_fail_closed() {
    let mut plan = plan();
    plan.event = WorkflowEvent::Push;
    plan.producers.context = Some(ProducerEventContext {
        reference: "refs/heads/main".to_owned(),
        default_branch: "main".to_owned(),
        protected: true,
        cargo_fallback: false,
    });
    for report in [
        None,
        Some(ProducerReport {
            identity: "forged".to_owned(),
            ..terminal()
        }),
    ] {
        let mut request = request("failure");
        request.producer_reports = report.into_iter().collect();
        let mut signals = Signals::default();
        required_evidence::check_required_evidence(
            &plan,
            &request,
            &mut signals,
            &mut BTreeSet::new(),
        );
        assert!(signals.planning_failed);
    }
    let mut skipped = request("skipped");
    skipped.producer_reports = vec![terminal()];
    let mut signals = Signals::default();
    required_evidence::check_required_evidence(&plan, &skipped, &mut signals, &mut BTreeSet::new());
    assert!(signals.planning_failed);

    let mut duplicated = request("failure");
    duplicated.producer_reports = vec![terminal(), terminal()];
    let mut signals = Signals::default();
    required_evidence::check_required_evidence(
        &plan,
        &duplicated,
        &mut signals,
        &mut BTreeSet::new(),
    );
    assert!(signals.planning_failed);
}

#[test]
fn terminal_availability_is_coherent_and_domain_specific() {
    assert!(!terminal_consistent(
        false,
        true,
        ProducerTerminalError::None,
        false
    ));
    assert!(!terminal_consistent(
        true,
        true,
        ProducerTerminalError::CacheNotPublished,
        true
    ));
    assert!(!terminal_consistent(
        false,
        false,
        ProducerTerminalError::PrivateOrAuthRequired,
        false
    ));
    assert!(terminal_consistent(
        false,
        false,
        ProducerTerminalError::PrivateOrAuthRequired,
        true
    ));
}

#[test]
fn producer_role_never_excuses_an_actual_task_owner_or_mandatory_availability() {
    let mut plan = plan();
    plan.obligations.push(PlanObligation {
        task_id: "stack/rust/unit/build/default".to_owned(),
        job_id: "tools-example".to_owned(),
        decision: ObligationDecision::Execute,
        reason: "selected".to_owned(),
        task_digest: "b3-".to_owned() + &"a".repeat(64),
        input_digest: "b3-".to_owned() + &"b".repeat(64),
        execution_identity: velnor_actions_contract::TaskExecutionIdentity::new(
            &velnor_actions_contract::digest_b3(b"fixture-graph"),
            &velnor_actions_contract::digest_b3(b"fixture-toolchain"),
            &velnor_actions_contract::digest_b3(b"fixture-mbx"),
            &velnor_actions_contract::digest_b3(b"fixture-platform"),
            "default",
        )
        .expect("execution identity"),
        closure_digest: "b3-".to_owned() + &"c".repeat(64),
        baseline_proof: None,
    });
    let mut signals = Signals::default();
    required_evidence::fold_jobs(&plan, &request("skipped"), &mut signals);
    assert!(signals.planning_failed);
    plan.obligations.clear();
    plan.producers.entries[0].policy = ProducerPolicy::Mandatory;
    let mut request = request("success");
    request.producer_reports = vec![ProducerReport {
        verified: true,
        error: ProducerTerminalError::CacheNotPublished,
        ..terminal()
    }];
    let mut signals = Signals::default();
    required_evidence::fold_jobs(&plan, &request, &mut signals);
    assert!(signals.failed);
}

#[test]
fn validator_identity_cannot_be_forged_into_a_producer_admission() {
    let mut plan = plan();
    plan.producers.entries[0].job_id = plan.producers.entries[0]
        .expected_job_id()
        .expect("canonical id");
    plan.validate().expect("valid isolated admission");
    plan.producers.entries[0].job_id = "alint".to_owned();
    assert!(plan.validate().is_err());
}

#[path = "required_producer_terminal_matrix_tests.rs"]
mod terminal_matrix;
