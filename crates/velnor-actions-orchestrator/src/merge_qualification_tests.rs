use super::*;

use std::collections::BTreeSet;

use velnor_actions_contract::{
    PlanGenerator, PlanRunner, QualificationDispatch, QualificationPhase, QualificationRunRef,
    RunnerSelection, Trust, WorkflowEvent, plan_id_for_run,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn context() -> QualificationDispatch {
    QualificationDispatch {
        campaign: "campaign-a".to_owned(),
        phase: QualificationPhase::Cold,
        repository: "owner/project".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: "owner/project/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: SHA.to_owned(),
        source_sha: SHA.to_owned(),
        run_id: 123,
        run_attempt: 1,
        predecessor: None,
    }
}

fn qualification_plan(context: QualificationDispatch) -> Plan {
    let run_key = "r123-a1";
    Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.to_owned(),
        plan_id: plan_id_for_run(run_key).expect("plan id"),
        base: None,
        head: SHA.to_owned(),
        event: WorkflowEvent::Qualification,
        qualification: Some(context),
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: velnor_actions_contract::PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "a".repeat(64),
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

fn request(actual_qualification: Option<QualificationDispatch>) -> MergeRequest {
    MergeRequest {
        schema: 1,
        run_key: "r123-a1".to_owned(),
        actual_event: Some(WorkflowEvent::Qualification),
        actual_qualification,
        candidate_attestation: None,
        plan: None,
        matrix: None,
        matrix_reports: Vec::new(),
        task_reports: Vec::new(),
        check_proofs: Vec::new(),
        required_job_ids: Vec::new(),
        required_jobs: Vec::new(),
        assembly_errors: Vec::new(),
        baseline_manifest: None,
        shard_proofs: Vec::new(),
        limits: None,
        reference_task_ids: None,
    }
}

fn is_coherent(plan: &Plan, request: &MergeRequest) -> bool {
    let mut signals = Signals::default();
    let mut reasons = BTreeSet::new();
    check_trust_coherence(plan, request, &mut signals, &mut reasons);
    !signals.planning_failed && !reasons.contains("trust_scope_mismatch")
}

#[test]
fn merge_requires_identical_qualification_provenance() {
    let planned = context();
    let plan = qualification_plan(planned.clone());
    assert!(is_coherent(&plan, &request(Some(planned.clone()))));

    let mutations: [fn(&mut QualificationDispatch); 12] = [
        |value| value.campaign = "campaign-b".to_owned(),
        |value| value.phase = QualificationPhase::Warm,
        |value| value.repository = "other/project".to_owned(),
        |value| value.default_branch = "stable".to_owned(),
        |value| value.git_ref = "refs/heads/feature".to_owned(),
        |value| value.ref_protected = false,
        |value| {
            value.workflow_ref =
                "other/project/.github/workflows/ci.yml@refs/heads/main".to_owned();
        },
        |value| value.workflow_sha = "1123456789abcdef0123456789abcdef01234567".to_owned(),
        |value| value.source_sha = "2123456789abcdef0123456789abcdef01234567".to_owned(),
        |value| value.run_id += 1,
        |value| value.run_attempt += 1,
        |value| {
            value.predecessor = Some(QualificationRunRef {
                run_id: 122,
                run_attempt: 1,
            });
        },
    ];
    for mutate in mutations {
        let mut actual = planned.clone();
        mutate(&mut actual);
        assert!(
            !is_coherent(&plan, &request(Some(actual.clone()))),
            "{actual:?}"
        );
    }
    assert!(!is_coherent(&plan, &request(None)));
}
