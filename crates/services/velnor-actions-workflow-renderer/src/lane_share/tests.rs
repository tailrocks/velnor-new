use std::collections::BTreeMap;

use velnor_actions_contract_config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepKind, StepRole, Trigger, WorkflowIr,
};

use super::{HOSTED_SUFFIX, SCALE_SUFFIX, share_lanes};
use crate::render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext};

const HOSTED_RUNS: &str = "ubuntu-26.04";
const LOGICAL_JOBS: usize = 21;

pub(super) fn ctx() -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: HOSTED_RUNS.to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: BTreeMap::new(),
    }
}

pub(super) fn workflow_ir() -> WorkflowIr {
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

fn scale_token() -> String {
    ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .expect("scale selector")
    .token()
}

pub(super) fn echo_step(index: usize, payload: &str) -> Step {
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

fn lane_job(display: &str, runs_on: &str, steps: Vec<Step>) -> Job {
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

fn checkout() -> Step {
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

pub(super) fn render_jobs(
    ir: &WorkflowIr,
    shared: &super::LaneShare,
    ctx: &RenderContext,
) -> Result<String, velnor_actions_workflow_steps::RenderError> {
    let document =
        crate::document::workflow_to_yaml(ir, shared, ctx, &std::collections::BTreeSet::new())?;
    let quoted = velnor_actions_workflow_tree::yaml::quote_run_values_in_yaml(document);
    velnor_actions_workflow_tree::marker::with_marker(
        &ctx.generator_version,
        &velnor_actions_workflow_tree::yaml::render_yaml(&quoted),
    )
}

pub(super) fn paired(steps: &[Step]) -> BTreeMap<String, Job> {
    let scale = scale_token();
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
    jobs
}

mod named_check_tests;
mod rejections;
mod render_tests;
mod shell_tests;
mod unpinned_tests;
