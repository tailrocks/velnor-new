//! Contract plan, report, and workflow cases.
use crate::impl_contract_ids::{MANIFEST, TASK, sample_entry};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    BaselineProof, CacheLayer, CacheOutcome, CacheResult, Concurrency, ContractError, FinalCounts,
    FinalReport, FinalStatus, Job, JobConclusion, JobTimeout, MatrixReport, MatrixStatus,
    NotSelectedReason, ObligationDecision, Permissions, Plan, PlanBaseline, PlanGenerator,
    PlanMatrix, PlanObligation, PlanPackage, PlanRunner, QualificationDispatch, QualificationPhase,
    RequiredJobResult, RunnerSelection, Step, StepKind, TaskReport, TaskStatus, Trigger, Trust,
    WorkflowEvent, WorkflowIr, artifact_id_for_matrix, artifact_id_for_plan, canonical_json_bytes,
    digest_b3, final_report_id_for_run, plan_id_for_run, run_key_for_ci, task_report_id_for_task,
    validate_final_report_id,
};

#[test]
fn plan_validates_sorting_and_matrix() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(3, 1);
    let entry = sample_entry(&run_key)?;
    let plan = Plan {
        schema: 1,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key)?,
        base: None,
        head: "ab".repeat(20),
        event: WorkflowEvent::PullRequest,
        qualification: None,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("no_entry"))?,
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "ab".repeat(32),
        },
        packages: vec![PlanPackage {
            package_id: "demo 0.1.0".to_owned(),
            name: "demo".to_owned(),
            manifest: MANIFEST.to_owned(),
            selected: true,
            reasons: vec!["changed".to_owned()],
            tasks: vec![TASK.to_owned()],
        }],
        obligations: vec![PlanObligation {
            task_id: TASK.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "changed".to_owned(),
            task_digest: digest_b3(b"task"),
            input_digest: digest_b3(b"inputs"),
            closure_digest: digest_b3(b"closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![TASK.to_owned()],
        warnings: vec![],
        edges: vec![],
    };
    plan.validate()?;
    let mut bad = plan.clone();
    bad.task_ids = vec![TASK.to_owned(), "aaa".to_owned()];
    assert!(bad.validate().is_err());
    let covered = PlanObligation {
        decision: ObligationDecision::CoveredByTrustedBaseline,
        baseline_proof: Some(BaselineProof::new(
            &"ab".repeat(20),
            99,
            4242,
            &artifact_id_for_plan(&run_key)?,
            &digest_b3(b"manifest"),
        )?),
        ..plan.obligations[0].clone()
    };
    let mut plan2 = plan.clone();
    plan2.obligations = vec![covered];
    plan2.validate()?;
    Ok(())
}

#[test]
fn qualification_plan_binds_source_run_and_forbids_coverage() -> Result<(), ContractError> {
    let mut plan = sample_qualification_plan()?;
    plan.validate()?;

    plan.qualification
        .as_mut()
        .expect("qualification context")
        .source_sha = "cd".repeat(20);
    assert!(plan.validate().is_err());
    plan.qualification
        .as_mut()
        .expect("qualification context")
        .source_sha = plan.head.clone();
    plan.qualification
        .as_mut()
        .expect("qualification context")
        .run_attempt = 2;
    assert!(plan.validate().is_err());

    let mut reused = plan.clone();
    reused.qualification.as_mut().expect("context").run_attempt = 1;
    reused.obligations[0].decision = ObligationDecision::ReusedFromTaskCache;
    assert!(reused.validate().is_err());
    Ok(())
}

fn sample_qualification_plan() -> Result<Plan, ContractError> {
    let run_key = run_key_for_ci(3, 1);
    let entry = sample_entry(&run_key)?;
    let head = "ab".repeat(20);
    Ok(Plan {
        schema: 1,
        run_key,
        plan_id: plan_id_for_run(&run_key_for_ci(3, 1))?,
        base: None,
        head: head.clone(),
        event: WorkflowEvent::Qualification,
        qualification: Some(QualificationDispatch {
            campaign: "campaign-2030".to_owned(),
            phase: QualificationPhase::Third,
            repository: "owner/project".to_owned(),
            default_branch: "main".to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            ref_protected: true,
            workflow_ref: "owner/project/.github/workflows/ci.yml@refs/heads/main".to_owned(),
            workflow_sha: head.clone(),
            source_sha: head,
            run_id: 3,
            run_attempt: 1,
            predecessor: Some(velnor_actions_contract::QualificationRunRef {
                run_id: 2,
                run_attempt: 1,
            }),
        }),
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("qualification_bypass"))?,
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "ab".repeat(32),
        },
        packages: vec![PlanPackage {
            package_id: "demo 0.1.0".to_owned(),
            name: "demo".to_owned(),
            manifest: MANIFEST.to_owned(),
            selected: true,
            reasons: vec!["qualification_full".to_owned()],
            tasks: vec![TASK.to_owned()],
        }],
        obligations: vec![PlanObligation {
            task_id: TASK.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "qualification_full".to_owned(),
            task_digest: digest_b3(b"task"),
            input_digest: digest_b3(b"inputs"),
            closure_digest: digest_b3(b"closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![TASK.to_owned()],
        warnings: vec![],
        edges: vec![],
    })
}

#[path = "impl_contract_report_validation.rs"]
mod validation_tests;
