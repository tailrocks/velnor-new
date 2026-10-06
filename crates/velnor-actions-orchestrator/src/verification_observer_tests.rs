//! Observer graph isolation and real validator-failure merge proof.

use std::{collections::BTreeMap, fs};

use velnor_actions_contract::{
    FinalReport, FinalStatus, Job, JobConclusion, JobTimeout, PermissionLevel, StepKind,
    VelnorConfig,
};

use super::{OBSERVER_JOB_ID, attach_observer};
use velnor_actions_workflow_renderer::render::{FINAL_JOB_ID, PLAN_JOB_ID};

fn config(dispatch: bool) -> VelnorConfig {
    let temp = tempfile::TempDir::new().expect("config directory");
    fs::create_dir(temp.path().join(".velnor")).expect("config parent");
    fs::write(
        temp.path().join(".velnor/config.toml"),
        format!(
            "schema = 1\n[workflow]\ndefault_branch = 'main'\n\
             [workflow.verification]\nschedule = '17 3 * * *'\n\
             workflow_dispatch = {dispatch}\nalert = true\n"
        ),
    )
    .expect("config file");
    crate::config::load_config(temp.path()).expect("valid config")
}

fn jobs(dispatch: bool) -> BTreeMap<String, Job> {
    let mut jobs = BTreeMap::from([(
        FINAL_JOB_ID.to_owned(),
        Job {
            cache_mode: None,
            display_name: "Required".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: JobTimeout::VALIDATOR,
            needs: vec!["plan".to_owned()],
            condition: None,
            permissions: None,
            tool_producer: None,
            mbx_producer: None,
            source_producer: None,
            native_pages_deploy: None,
            native_publish: None,
            outputs: Vec::new(),
            environment: None,
            steps: vec![],
        },
    )]);
    let mut plan = jobs[FINAL_JOB_ID].clone();
    plan.display_name = "Plan".to_owned();
    plan.needs.clear();
    plan.steps = vec![velnor_actions_workflow_renderer::plan_step()];
    jobs.insert(PLAN_JOB_ID.to_owned(), plan);
    attach_observer(
        &mut jobs,
        &config(dispatch),
        "tailrocks/velnor-new",
        "main",
        "ubuntu-26.04",
    )
    .expect("observer graph");
    jobs
}

#[test]
fn dispatch_simulation_fails_existing_plan_before_plan_operation() {
    let jobs = jobs(true);
    assert_eq!(jobs.len(), 3, "simulation must not add a runner job");
    assert_eq!(jobs[FINAL_JOB_ID].needs, [PLAN_JOB_ID]);
    assert!(
        !jobs[FINAL_JOB_ID]
            .needs
            .iter()
            .any(|id| id == OBSERVER_JOB_ID)
    );
    assert_eq!(jobs[OBSERVER_JOB_ID].needs, [FINAL_JOB_ID]);
    let plan = &jobs[PLAN_JOB_ID];
    assert!(plan.needs.is_empty());
    assert!(plan.condition.is_none());
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(
        plan.steps[0].condition.as_deref(),
        Some(
            "github.event_name == 'workflow_dispatch' && toJSON(inputs.simulate_failure) == 'true'"
        )
    );
    assert!(plan.steps[1].condition.is_none());
    let StepKind::Internal { operation } = &plan.steps[1].kind else {
        panic!("fixed Plan operation must follow simulation")
    };
    assert_eq!(operation, "plan-v1");
    let StepKind::Shell { run, .. } = &plan.steps[0].kind else {
        panic!("simulation must execute a real failing shell command")
    };
    assert_eq!(run.first().map(String::as_str), Some("env"));
    assert_eq!(run.last().map(String::as_str), Some("false"));
    let unset = &run[1..run.len() - 1];
    assert_eq!(unset.len() % 2, 0);
    for pair in unset.chunks_exact(2) {
        assert_eq!(pair[0], "-u");
        assert!(
            velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS
                .contains(&pair[1].as_str())
        );
    }
}

#[test]
fn schedule_only_has_no_dispatch_simulation_dependency() {
    let jobs = jobs(false);
    assert_eq!(jobs.len(), 3);
    assert_eq!(jobs[PLAN_JOB_ID].steps.len(), 1);
    assert_eq!(
        jobs[PLAN_JOB_ID].steps[0],
        velnor_actions_workflow_renderer::plan_step()
    );
    assert_eq!(jobs[FINAL_JOB_ID].needs, ["plan"]);
    assert_eq!(jobs[OBSERVER_JOB_ID].needs, [FINAL_JOB_ID]);
}

#[test]
fn only_observer_has_issue_write_and_protected_environment() {
    let jobs = jobs(true);
    {
        let job = &jobs[OBSERVER_JOB_ID];
        let permissions = job.permissions.as_ref().expect("explicit permissions");
        assert_eq!(permissions.contents, PermissionLevel::None);
        assert_eq!(permissions.pull_requests, PermissionLevel::None);
        assert_eq!(permissions.id_token, PermissionLevel::None);
        assert_eq!(permissions.actions, PermissionLevel::None);
        assert_eq!(permissions.issues, PermissionLevel::Write);
    }
    assert_eq!(
        jobs[OBSERVER_JOB_ID].environment.as_deref(),
        Some("verification-alerts")
    );
    assert!(jobs[PLAN_JOB_ID].environment.is_none());
    assert!(jobs[FINAL_JOB_ID].environment.is_none());
    let condition = jobs[OBSERVER_JOB_ID]
        .condition
        .as_deref()
        .expect("trust gate");
    for gate in [
        "always()",
        "github.repository == 'tailrocks/velnor-new'",
        "github.ref == 'refs/heads/main'",
        "github.ref_protected == true",
        "github.event_name == 'schedule'",
        "github.event_name == 'workflow_dispatch'",
        "needs.required.result != 'success'",
    ] {
        assert!(condition.contains(gate), "missing {gate}: {condition}");
    }
}

#[test]
fn observer_has_only_isolated_tools_and_native_python() {
    let jobs = jobs(true);
    let steps = &jobs[OBSERVER_JOB_ID].steps;
    assert_eq!(steps.len(), 3);
    let operations: Vec<_> = steps
        .iter()
        .map(|step| match &step.kind {
            StepKind::SourceBoundHelper { invocation, .. } => invocation.descriptor().operation(),
            StepKind::Action { .. } => panic!("observer cannot execute actions"),
            StepKind::Shell { .. } => panic!("observer cannot execute raw shell"),
            StepKind::Internal { .. } => panic!("observer cannot execute repository operations"),
        })
        .collect();
    assert_eq!(
        operations,
        [
            velnor_actions_contract::SourceBoundOperation::MiseBootstrap,
            velnor_actions_contract::SourceBoundOperation::MiseToolPrepare,
            velnor_actions_contract::SourceBoundOperation::VerificationObserver,
        ]
    );
    let StepKind::SourceBoundHelper { env, .. } = &steps[2].kind else {
        panic!("observer must use source-bound Python")
    };
    assert_eq!(env["REQUIRED_RESULT"], "${{ needs.required.result }}");
    assert_eq!(env["GH_TOKEN"], "${{ github.token }}");
}

#[test]
fn simulated_plan_failure_reaches_real_required_diagnostic() {
    let temp = tempfile::TempDir::new().expect("empty artifact directory");
    let request = crate::merge_request::assemble_with_needs(
        "local",
        temp.path(),
        Some(r#"{"plan":{"result":"failure","outputs":{}}}"#),
        Some(r#"["plan"]"#),
        Some("workflow_dispatch"),
        Some(r#"{"inputs":{"scope":"full","simulate_failure":true}}"#),
    )
    .expect("actual Plan failure assembly");
    let envelope: serde_json::Value = serde_json::from_str(&request).expect("request JSON");
    assert!(
        envelope["plan"].is_null(),
        "failed Plan produces no artifact"
    );
    assert!(envelope["matrix"].is_null());
    assert_eq!(
        envelope["required_job_ids"],
        serde_json::json!([PLAN_JOB_ID])
    );
    let report: FinalReport = serde_json::from_str(
        &crate::merge::merge_internal(&request).expect("merge actual Plan failure"),
    )
    .expect("real final report");
    assert_eq!(report.status, FinalStatus::PlanningFailed, "{report:?}");
    assert_eq!(report.required_job_results.len(), 1);
    assert_eq!(report.required_job_results[0].job_id, PLAN_JOB_ID);
    assert_eq!(
        report.required_job_results[0].conclusion,
        JobConclusion::Failure
    );
    assert!(
        report
            .miss_reasons
            .iter()
            .any(|reason| reason == "source_missing")
    );
    report.validate().expect("diagnostic contract");
}
