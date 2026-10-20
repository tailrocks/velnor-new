use std::collections::BTreeMap;
use std::{error::Error, io};

use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepId, StepKind, StepRole, Trigger,
    WorkflowIr,
};

use super::{
    DOWNLOAD_PLAN_CONDITION, PLAN_FAILURE_GUARD_CONDITION, PLAN_FAILURE_GUARD_NAME,
    check_preseed_closure, require_successful_plan_for_download,
};

fn role_step(role: StepRole) -> Step {
    Step {
        name: format!("{role:?}"),
        id: None,
        role: Some(role),
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

fn consumer_steps() -> Vec<Step> {
    [
        StepRole::PreseedDownload,
        StepRole::PreseedVerifyManifest,
        StepRole::PreseedStage,
    ]
    .into_iter()
    .map(role_step)
    .collect()
}

fn plan_job(steps: Vec<Step>) -> Job {
    Job {
        services: Vec::new(),
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        steps,
    }
}

fn consumer_job(needs: Vec<String>, condition: Option<&str>, steps: Vec<Step>) -> Job {
    Job {
        services: Vec::new(),
        display_name: "Helper consumer".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::VALIDATOR,
        needs,
        condition: condition.map(str::to_owned),
        permissions: None,
        cache_mode: None,
        environment: None,
        steps,
    }
}

fn producer_steps() -> Vec<Step> {
    [
        StepRole::PreseedBuild,
        StepRole::PreseedManifest,
        StepRole::PreseedUpload,
        StepRole::PreseedStage,
    ]
    .into_iter()
    .map(role_step)
    .collect()
}

fn valid_role_step(role: StepRole) -> Step {
    let kind = match role {
        StepRole::PreseedDownload => StepKind::Action {
            uses: "actions/download-artifact@v4".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
        StepRole::PreseedUpload => StepKind::Action {
            uses: "actions/upload-artifact@v4".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
        _ => StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    };
    Step {
        name: format!("{role:?}"),
        id: (role == StepRole::PreseedStage).then_some(StepId::PreseedStage),
        role: Some(role),
        condition: None,
        parallel: false,
        kind,
    }
}

fn workflow_ir(jobs: BTreeMap<String, Job>) -> WorkflowIr {
    WorkflowIr {
        name: "Pre-seed graph test".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "preseed-graph-test".to_owned(),
            cancel_in_progress: "false".to_owned(),
        },
        jobs,
    }
}

#[test]
fn every_consumer_gets_plan_edge_without_losing_existing_needs() -> Result<(), Box<dyn Error>> {
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), plan_job(producer_steps())),
        (
            "existing-check".to_owned(),
            consumer_job(Vec::new(), None, Vec::new()),
        ),
        (
            "static-policy-check".to_owned(),
            consumer_job(vec!["existing-check".to_owned()], None, consumer_steps()),
        ),
    ]);

    let result = check_preseed_closure(&mut jobs, true);
    assert!(result.is_ok(), "{result:?}");
    let consumer = jobs
        .get("static-policy-check")
        .ok_or_else(|| io::Error::other("consumer job is present"))?;
    assert_eq!(
        consumer.needs,
        ["existing-check".to_owned(), "plan".to_owned()]
    );
    let download = consumer
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::PreseedDownload))
        .ok_or_else(|| io::Error::other("helper download remains present"))?;
    assert_eq!(download.condition.as_deref(), Some(DOWNLOAD_PLAN_CONDITION));
    assert!(
        !consumer
            .steps
            .iter()
            .any(|step| step.name == PLAN_FAILURE_GUARD_NAME)
    );
    Ok(())
}

#[test]
fn always_consumer_fails_on_missing_plan_before_helper_download() -> Result<(), Box<dyn Error>> {
    let mut steps = consumer_steps();
    steps.push(Step {
        name: "Merge reports".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: "merge-v1".to_owned(),
            env: BTreeMap::new(),
        },
    });
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), plan_job(producer_steps())),
        (
            "required".to_owned(),
            consumer_job(
                vec!["plan".to_owned(), "other-check".to_owned()],
                Some("always()"),
                steps,
            ),
        ),
    ]);

    let result = check_preseed_closure(&mut jobs, true);
    assert!(result.is_ok(), "{result:?}");
    let required = jobs
        .get("required")
        .ok_or_else(|| io::Error::other("required job is present"))?;
    assert_eq!(required.condition.as_deref(), Some("always()"));
    assert_eq!(
        required.needs,
        ["plan".to_owned(), "other-check".to_owned()]
    );
    let guard = required
        .steps
        .first()
        .ok_or_else(|| io::Error::other("plan result guard precedes download"))?;
    assert_eq!(guard.name, PLAN_FAILURE_GUARD_NAME);
    assert_eq!(
        guard.condition.as_deref(),
        Some(PLAN_FAILURE_GUARD_CONDITION)
    );
    let StepKind::Shell { run, env } = &guard.kind else {
        return Err(io::Error::other("plan result guard is a fixed shell check").into());
    };
    assert!(run.join(" ").contains("exit 1"));
    assert!(
        env.values()
            .all(|value| !value.contains("needs.plan.result"))
    );
    let download_at = required
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::PreseedDownload))
        .ok_or_else(|| io::Error::other("helper download remains present"))?;
    assert!(download_at > 0);
    assert_eq!(
        required.steps[download_at].condition.as_deref(),
        Some(DOWNLOAD_PLAN_CONDITION)
    );
    assert!(
        required
            .steps
            .iter()
            .any(|step| matches!(&step.kind, StepKind::Internal { operation, .. } if operation == "merge-v1")),
        "aggregate step remains in the job after the guard"
    );
    Ok(())
}

#[test]
fn missing_or_self_plan_never_creates_a_plan_edge() -> Result<(), Box<dyn Error>> {
    let consumer = consumer_job(vec!["existing-check".to_owned()], None, consumer_steps());
    let mut missing_plan = BTreeMap::from([("tool-check".to_owned(), consumer.clone())]);
    assert!(matches!(
        check_preseed_closure(&mut missing_plan, true),
        Err(crate::RenderError::InvalidWorkflow(problem)) if problem == "preseed_plan_missing"
    ));
    assert_eq!(
        missing_plan
            .get("tool-check")
            .ok_or_else(|| io::Error::other("consumer remains"))?
            .needs,
        ["existing-check".to_owned()]
    );

    let mut self_consuming_plan_steps = producer_steps();
    self_consuming_plan_steps.extend(consumer_steps());
    let mut self_plan = BTreeMap::from([("plan".to_owned(), plan_job(self_consuming_plan_steps))]);
    assert!(matches!(
        check_preseed_closure(&mut self_plan, true),
        Err(crate::RenderError::InvalidWorkflow(problem))
            if problem == "preseed_plan_consumes_own_artifact"
    ));
    assert_eq!(
        self_plan
            .get("plan")
            .ok_or_else(|| io::Error::other("plan remains"))?
            .needs
            .as_slice(),
        &[] as &[String]
    );
    Ok(())
}

#[test]
fn download_gate_wraps_prior_condition_even_when_it_ends_with_plan_gate() {
    let prior = "failure() || success() && needs.plan.result == 'success'";
    let mut step = valid_role_step(StepRole::PreseedDownload);
    step.condition = Some(prior.to_owned());

    require_successful_plan_for_download(&mut step);

    assert_eq!(
        step.condition.as_deref(),
        Some(format!("({prior}) && {DOWNLOAD_PLAN_CONDITION}").as_str())
    );
}

#[test]
fn adding_plan_need_rejects_a_transitive_dependency_cycle() {
    let mut plan = plan_job(
        [
            StepRole::PreseedBuild,
            StepRole::PreseedManifest,
            StepRole::PreseedUpload,
            StepRole::PreseedStage,
        ]
        .into_iter()
        .map(valid_role_step)
        .collect(),
    );
    plan.needs.push("helper-check".to_owned());
    let helper = consumer_job(
        Vec::new(),
        None,
        [
            StepRole::PreseedDownload,
            StepRole::PreseedVerifyManifest,
            StepRole::PreseedStage,
        ]
        .into_iter()
        .map(valid_role_step)
        .collect(),
    );
    let mut jobs = BTreeMap::from([
        ("plan".to_owned(), plan),
        ("helper-check".to_owned(), helper),
    ]);

    assert!(check_preseed_closure(&mut jobs, true).is_ok());
    assert_eq!(
        workflow_ir(jobs).validate(),
        Err(velnor_actions_contract::ContractError::identity(
            "workflow.jobs",
            "dependency_cycle"
        ))
    );
}
