//! End-to-end optional verification observer wiring.

use std::collections::BTreeSet;

use tempfile::TempDir;
use velnor_actions_contract::workflow::ir::DispatchInputType;
use velnor_actions_contract::{
    NativeCredentialScope, PermissionLevel, SourceBoundOperation, StepKind,
};
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::{
    WORKFLOW_PATH,
    render::{FINAL_JOB_ID, PLAN_JOB_ID},
    steps::PLAN_OPERATION,
};

use crate::impl_common::{TestResult, config_with_branch, err_of, git, make_repo};

const OBSERVER_JOB_ID: &str = "verification-observer";
const SIMULATION_STEP_NAME: &str = "Simulate verification failure";
const SIMULATION_CONDITION: &str =
    "github.event_name == 'workflow_dispatch' && toJSON(inputs.simulate_failure) == 'true'";

fn verification_config(branch: &str, settings: &str) -> String {
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"{branch}\"\n[workflow.verification]\n{settings}"
    )
}

fn alert_repo(branch: &str, settings: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(&verification_config(branch, settings))?;
    git(
        &["remote", "add", "origin", "https://github.com/o/r.git"],
        repo.path(),
    )?;
    Ok(repo)
}

fn assert_source_bound_observer(
    prep: &GenerationPreparation,
    observer: &velnor_actions_contract::Job,
) -> TestResult {
    let helper = observer.steps.get(2).ok_or("observer helper missing")?;
    let StepKind::SourceBoundHelper { invocation, env } = &helper.kind else {
        return Err("observer helper must be source-bound".into());
    };
    assert_eq!(
        invocation.descriptor().operation(),
        SourceBoundOperation::VerificationObserver
    );
    assert!(invocation.args().is_empty());
    assert_eq!(
        env.get("APPROVED_REPOSITORY").map(String::as_str),
        Some("o/r")
    );
    let record = prep
        .workflow
        .context
        .source_helpers
        .iter()
        .find(|record| record.invocation() == invocation && record.environment() == env)
        .ok_or("observer helper registry record missing")?;
    let recipe = record
        .execution_recipe()
        .ok_or("observer helper execution recipe missing")?;
    assert_eq!(
        recipe.credential_scope(),
        NativeCredentialScope::GithubIssueWrite
    );
    Ok(())
}

#[test]
fn schedule_alert_adds_protected_observer_without_dispatch_payload() -> TestResult {
    let repo = alert_repo("testmain", "schedule = \"17 3 * * *\"\nalert = true\n")?;
    let prep = prepare(repo.path())?;
    let triggers = &prep.workflow.ir.triggers;
    assert!(triggers.schedule.is_some());
    assert!(triggers.workflow_dispatch.is_none());

    let jobs = &prep.workflow.ir.jobs;
    let observer = jobs.get(OBSERVER_JOB_ID).ok_or("observer job missing")?;
    assert_eq!(observer.needs, vec![FINAL_JOB_ID.to_owned()]);
    assert_eq!(observer.environment.as_deref(), Some("verification-alerts"));
    assert_eq!(
        observer
            .permissions
            .as_ref()
            .map(|permissions| permissions.issues),
        Some(PermissionLevel::Write)
    );
    let condition = observer
        .condition
        .as_deref()
        .ok_or("observer condition missing")?;
    for fragment in [
        "github.repository == 'o/r'",
        "github.ref == 'refs/heads/testmain'",
        "github.ref_protected == true",
        "github.event_name == 'schedule'",
        "github.event_name == 'workflow_dispatch'",
        "needs.required.result != 'success'",
    ] {
        assert!(
            condition.contains(fragment),
            "missing {fragment}: {condition}"
        );
    }
    assert_source_bound_observer(&prep, observer)?;
    assert!(!jobs.contains_key("verification-simulation"));
    let plan = jobs.get(PLAN_JOB_ID).ok_or("plan job missing")?;
    assert!(
        !plan
            .steps
            .iter()
            .any(|step| step.name == SIMULATION_STEP_NAME)
    );

    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("main workflow missing")?;
    assert!(yaml.contains("verification-observer:"), "observer:\n{yaml}");
    assert!(yaml.contains("issues: write"), "permission:\n{yaml}");
    assert!(
        !yaml.contains("simulate_failure:"),
        "dispatch input:\n{yaml}"
    );
    Ok(())
}

fn assert_dispatch_inputs(prep: &GenerationPreparation) -> TestResult {
    let dispatch = prep
        .workflow
        .ir
        .triggers
        .workflow_dispatch
        .as_ref()
        .ok_or("dispatch trigger missing")?;
    let names: Vec<&str> = dispatch
        .inputs
        .iter()
        .map(|input| input.name.as_str())
        .collect();
    assert_eq!(names, vec!["base_sha", "scope", "simulate_failure"]);
    let simulation = dispatch
        .inputs
        .iter()
        .find(|input| input.name == "simulate_failure")
        .ok_or("simulation input missing")?;
    assert_eq!(simulation.input_type, DispatchInputType::Boolean);
    assert!(!simulation.required);
    assert_eq!(simulation.default.as_deref(), Some("false"));
    Ok(())
}

fn assert_plan_simulation(prep: &GenerationPreparation) -> TestResult {
    let jobs = &prep.workflow.ir.jobs;
    assert!(jobs.contains_key(OBSERVER_JOB_ID));
    assert!(!jobs.contains_key("verification-simulation"));
    let observer = jobs.get(OBSERVER_JOB_ID).ok_or("observer job missing")?;
    assert_eq!(observer.needs, vec![FINAL_JOB_ID.to_owned()]);
    let required = jobs.get(FINAL_JOB_ID).ok_or("required job missing")?;
    assert!(
        required.needs.iter().any(|job| job == PLAN_JOB_ID),
        "required must retain plan dependency: {:?}",
        required.needs
    );
    assert!(
        !required.needs.iter().any(|job| job == OBSERVER_JOB_ID),
        "observer cannot be required: {:?}",
        required.needs
    );
    let plan = jobs.get(PLAN_JOB_ID).ok_or("plan job missing")?;
    let simulation_index = plan
        .steps
        .iter()
        .position(|step| step.name == SIMULATION_STEP_NAME)
        .ok_or("simulation step missing")?;
    assert_eq!(
        plan.steps[simulation_index].condition.as_deref(),
        Some(SIMULATION_CONDITION)
    );
    let StepKind::Shell { run, .. } = &plan.steps[simulation_index].kind else {
        return Err("simulation step must be a shell step".into());
    };
    assert_eq!(run.last().map(String::as_str), Some("false"));
    let plan_operation_index = plan
        .steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::Internal { operation } if operation == PLAN_OPERATION)
        })
        .ok_or("plan operation missing")?;
    assert_eq!(simulation_index + 1, plan_operation_index);
    Ok(())
}

fn assert_only_observer_job(
    baseline: &GenerationPreparation,
    alert: &GenerationPreparation,
) -> TestResult {
    let baseline_ids: BTreeSet<String> = baseline.workflow.ir.jobs.keys().cloned().collect();
    let mut expected_ids = baseline_ids.clone();
    expected_ids.insert(OBSERVER_JOB_ID.to_owned());
    let generated_ids: BTreeSet<String> = alert.workflow.ir.jobs.keys().cloned().collect();
    assert_eq!(generated_ids.len(), expected_ids.len());
    assert_eq!(
        generated_ids
            .iter()
            .map(|id| id.as_str())
            .collect::<BTreeSet<_>>(),
        expected_ids
            .iter()
            .map(|id| id.as_str())
            .collect::<BTreeSet<_>>()
    );
    Ok(())
}

fn assert_dispatch_yaml(prep: &GenerationPreparation) -> TestResult {
    let tree = render_staged_tree(prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("main workflow missing")?;
    assert!(yaml.contains("simulate_failure:"), "input:\n{yaml}");
    assert!(yaml.contains("type: boolean"), "input type:\n{yaml}");
    assert!(yaml.contains("default: false"), "input default:\n{yaml}");
    assert!(
        !yaml.contains("verification-simulation:"),
        "extra job:\n{yaml}"
    );
    Ok(())
}

#[test]
fn dispatch_alert_adds_boolean_simulation_before_plan_and_only_observer_job() -> TestResult {
    let baseline_repo = alert_repo("testmain", "workflow_dispatch = true\n")?;
    let baseline = prepare(baseline_repo.path())?;
    let repo = alert_repo("testmain", "workflow_dispatch = true\nalert = true\n")?;
    let prep = prepare(repo.path())?;
    assert_dispatch_inputs(&prep)?;
    assert_plan_simulation(&prep)?;
    assert_only_observer_job(&baseline, &prep)?;
    assert_dispatch_yaml(&prep)?;
    Ok(())
}

#[test]
fn verification_defaults_have_no_observer_or_issue_write() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    assert!(!prep.workflow.ir.jobs.contains_key(OBSERVER_JOB_ID));
    assert!(prep.workflow.ir.jobs.values().all(|job| {
        job.permissions
            .as_ref()
            .is_none_or(|permissions| permissions.issues != PermissionLevel::Write)
    }));
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("main workflow missing")?;
    assert!(!yaml.contains("issues: write"), "issue write:\n{yaml}");
    Ok(())
}

#[test]
fn alert_without_native_trigger_is_rejected() -> TestResult {
    let repo = make_repo(&verification_config("testmain", "alert = true\n"))?;
    let error = err_of(prepare(repo.path()), "alert without trigger")?;
    assert!(error.to_string().contains("requires_schedule_or_dispatch"));
    Ok(())
}

#[test]
fn alert_requires_a_github_origin() -> TestResult {
    let repo = alert_repo("testmain", "schedule = \"17 3 * * *\"\nalert = true\n")?;
    git(
        &["remote", "set-url", "origin", "https://gitlab.com/o/r.git"],
        repo.path(),
    )?;
    let error = err_of(prepare(repo.path()), "non-GitHub origin")?;
    assert!(
        error.to_string().contains("release_origin_not_github"),
        "{error}"
    );
    Ok(())
}

#[test]
fn alert_rejects_branch_expression_before_condition_interpolation() -> TestResult {
    let repo = alert_repo(
        "main'||true||'main",
        "schedule = \"17 3 * * *\"\nalert = true\n",
    )?;
    let error = err_of(prepare(repo.path()), "branch expression")?;
    let text = error.to_string();
    assert!(
        text.contains("invalid_observer_branch") || text.contains("malformed_branch"),
        "{error}"
    );
    Ok(())
}
