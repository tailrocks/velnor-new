//! Contract qualification-dispatch plan cases.
use crate::impl_contract_ids::{MANIFEST, TASK, sample_entry};
use velnor_actions_contract::{
    ContractError, ObligationDecision, Plan, PlanGenerator, PlanMatrix, PlanObligation,
    PlanPackage, PlanRunner, QualificationDispatch, QualificationDispatchMode, QualificationPhase,
    RunnerSelection, TaskBindingDigest, Trust, WorkflowEvent, digest_b3, plan_id_for_run,
    run_key_for_ci,
};

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
        schema: Plan::SCHEMA,
        run_key,
        plan_id: plan_id_for_run(&run_key_for_ci(3, 1))?,
        base: None,
        head: head.clone(),
        event: WorkflowEvent::Qualification,
        qualification: Some(QualificationDispatch {
            mode: QualificationDispatchMode::Controlled,
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
            scale_set_image_source_sha: None,
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
            task_binding_digest: TaskBindingDigest::new(digest_b3(b"task"))?,
            input_digest: digest_b3(b"inputs"),
            closure_digest: digest_b3(b"closure"),
            graph_digest: digest_b3(b"graph"),
            mbx_digest: digest_b3(b"mbx"),
            profile: "default".to_owned(),
            job_id: entry.job_id.clone(),
        }],
        jobs: vec![velnor_actions_contract::PlanJob {
            job_id: entry.job_id.clone(),
            disposition: velnor_actions_contract::JobDisposition::Run,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![TASK.to_owned()],
        warnings: vec![],
        edges: vec![],
    })
}
