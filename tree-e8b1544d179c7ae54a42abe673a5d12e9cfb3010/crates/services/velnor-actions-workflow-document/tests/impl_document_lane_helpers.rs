//! Lane-share test helpers (duplicated per test target).

use std::collections::BTreeMap;

use velnor_actions_contract_config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepKind, StepRole, Trigger, WorkflowIr,
};
use velnor_actions_workflow_steps::RenderError;

use velnor_actions_contract_workflow::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_jobs::{
    RenderContext,
    context::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP},
};

pub(crate) const HOSTED_RUNS: &str = "ubuntu-26.04";
const LOGICAL_JOBS: usize = 21;

pub(crate) fn ctx() -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: HOSTED_RUNS.to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        rust_policy: None,
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: BTreeMap::new(),
    }
}

pub(crate) fn workflow_ir() -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs: BTreeMap::new(),
    }
}

pub(crate) fn scale_token() -> Result<String, RenderError> {
    Ok(ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(RenderError::Contract)?
    .token())
}

pub(crate) fn echo_step(index: usize, payload: &str) -> Step {
    Step {
        name: format!("echo {index}"),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["echo".to_owned(), payload.to_owned()],
            env: BTreeMap::new(),
        },
    }
}

pub(crate) fn lane_job(display: &str, runs_on: &str, steps: Vec<Step>) -> Job {
    Job {
        display_name: display.to_owned(),
        runs_on: runs_on.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        check_runner: None,
        steps,
    }
}

pub(crate) fn checkout() -> Step {
    Step {
        name: "Checkout".to_owned(),
        id: None,
        role: Some(StepRole::Checkout),
        condition: None,
        kind: StepKind::Action {
            uses: ctx().checkout_uses,
            with: BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]),
            env: BTreeMap::new(),
        },
    }
}

pub(crate) fn render_jobs(
    ir: &WorkflowIr,
    shared: &velnor_actions_workflow_document::lane_share::LaneShare,
    ctx: &RenderContext,
) -> Result<String, velnor_actions_workflow_steps::RenderError> {
    let document = velnor_actions_workflow_document::document::workflow_to_yaml(
        ir,
        shared,
        ctx,
        &std::collections::BTreeSet::new(),
    )?;
    let quoted = velnor_actions_workflow_tree::yaml::quote_run_values_in_yaml(document);
    velnor_actions_workflow_tree::marker::with_marker(
        &ctx.generator_version,
        &velnor_actions_workflow_tree::yaml::render_yaml(&quoted),
    )
}

pub(crate) fn paired(steps: &[Step]) -> Result<BTreeMap<String, Job>, RenderError> {
    let scale = scale_token()?;
    let mut lane_steps = vec![checkout()];
    lane_steps.extend_from_slice(steps);
    let mut jobs = BTreeMap::new();
    for index in 0..LOGICAL_JOBS {
        let logical = format!("rust-{index}");
        jobs.insert(
            format!("{logical}{HOSTED_SUFFIX}"),
            lane_job(
                &format!("{logical} hosted"),
                HOSTED_RUNS,
                lane_steps.clone(),
            ),
        );
        jobs.insert(
            format!("{logical}{SCALE_SUFFIX}"),
            lane_job(&format!("{logical} local"), &scale, lane_steps.clone()),
        );
    }
    Ok(jobs)
}
