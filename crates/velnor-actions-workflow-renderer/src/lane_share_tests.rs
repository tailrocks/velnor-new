use std::collections::BTreeMap;

use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, SCALE_SET_NAME, ScaleSetSelector, Step, StepKind,
    StepRole, Trigger, VELNOR_LABEL, WorkflowIr,
};

use super::{HOSTED_SUFFIX, SCALE_SUFFIX, share_lanes};
use crate::RenderError;
use crate::render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext};

#[path = "lane_share_cache_tests.rs"]
mod cache_tests;

const HOSTED_RUNS: &str = "ubuntu-26.04";
const LOGICAL_JOBS: usize = 21;

pub(super) fn ctx() -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: HOSTED_RUNS.to_owned(),
        scale_set_selector: None,
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
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
) -> Result<String, crate::RenderError> {
    let document =
        crate::document::workflow_to_yaml(ir, shared, ctx, &std::collections::BTreeSet::new())?;
    crate::marker::with_marker(&ctx.generator_version, &crate::yaml::render_yaml(&document))
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

#[test]
fn differing_lane_bodies_fail_closed() {
    let step = echo_step(0, "one");
    let mut jobs = paired(&[step]);
    jobs.get_mut("rust-0__hosted").expect("hosted").steps.pop();
    let err = share_lanes(&jobs, &ctx()).expect_err("differs");
    assert!(
        matches!(err, RenderError::InvalidWorkflow(ref problem) if problem == "lane_body_differs:rust-0"),
        "{err}"
    );
}

fn assert_lane_pair_rejected(jobs: &BTreeMap<String, Job>) {
    let err = share_lanes(jobs, &ctx()).expect_err("invalid lane checkout");
    assert!(
        matches!(err, RenderError::InvalidWorkflow(ref problem) if problem == "lane_body_differs:rust-0"),
        "{err}"
    );
}

#[test]
fn missing_lane_checkout_fails_closed() {
    let mut jobs = paired(&[echo_step(0, "one")]);
    jobs.get_mut("rust-0__hosted")
        .expect("hosted")
        .steps
        .remove(0);
    assert_lane_pair_rejected(&jobs);
}

#[test]
fn nonleading_lane_checkout_fails_closed() {
    let mut jobs = paired(&[echo_step(0, "one")]);
    jobs.get_mut("rust-0__hosted")
        .expect("hosted")
        .steps
        .swap(0, 1);
    assert_lane_pair_rejected(&jobs);
}

#[test]
fn duplicate_lane_checkout_fails_closed() {
    let mut jobs = paired(&[echo_step(0, "one")]);
    jobs.get_mut("rust-0__hosted")
        .expect("hosted")
        .steps
        .push(checkout());
    assert_lane_pair_rejected(&jobs);
}

#[test]
fn mismatched_lane_checkout_inputs_fail_closed() {
    let mut jobs = paired(&[echo_step(0, "one")]);
    let local = jobs.get_mut("rust-0__local").expect("local");
    let StepKind::Action { with, .. } = &mut local.steps[0].kind else {
        panic!("checkout action");
    };
    with.insert("fetch-depth".to_owned(), "0".to_owned());
    assert_lane_pair_rejected(&jobs);
}

#[test]
fn matching_lane_checkout_inputs_are_carried_outside_the_composite() {
    let mut jobs = paired(&[echo_step(0, "one")]);
    for id in ["rust-0__hosted", "rust-0__local"] {
        let StepKind::Action { with, .. } = &mut jobs.get_mut(id).expect("lane").steps[0].kind
        else {
            panic!("checkout action");
        };
        with.insert("fetch-depth".to_owned(), "0".to_owned());
    }
    let expected = jobs
        .get("rust-0__hosted")
        .expect("hosted")
        .steps
        .first()
        .expect("checkout")
        .clone();
    let shared = share_lanes(&jobs, &ctx()).expect("share");
    assert_eq!(shared.checkouts.get("rust-0__hosted"), Some(&expected));
    assert_eq!(shared.checkouts.get("rust-0__local"), Some(&expected));
    let action = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/rust-0/action.yml")
        .expect("composite");
    assert!(!action.bytes.contains("Checkout"));
    let yaml = render_jobs(&workflow_ir(), &shared, &ctx()).expect("yaml");
    assert!(yaml.contains("fetch-depth:"));
}

#[test]
fn unsafe_logical_id_fails_closed() {
    let step = echo_step(0, "one");
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "rust.0__hosted".to_owned(),
        lane_job("hosted", HOSTED_RUNS, vec![step.clone()]),
    );
    jobs.insert(
        "rust.0__local".to_owned(),
        lane_job("local", &scale_token(), vec![step]),
    );
    let err = share_lanes(&jobs, &ctx()).expect_err("bad id");
    assert!(
        matches!(err, RenderError::InvalidWorkflow(ref problem) if problem == "bad_lane_id:rust.0__hosted"),
        "{err}"
    );
}

#[test]
fn unpaired_jobs_stay_inline() {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "actionlint".to_owned(),
        lane_job("actionlint", HOSTED_RUNS, vec![echo_step(0, "one")]),
    );
    let shared = share_lanes(&jobs, &ctx()).expect("share");
    assert!(shared.calls.is_empty());
    assert_eq!(shared.files, [] as [tree::RenderedFile; 0]);
    let kept = shared.jobs.get("actionlint").expect("actionlint");
    assert_eq!(kept.steps.len(), 1);
}

#[test]
fn elected_save_stays_on_the_winner_job() {
    let mut jobs = paired(&[echo_step(0, "one")]);
    let expected_checkout = jobs
        .get("rust-0__hosted")
        .expect("hosted")
        .steps
        .first()
        .expect("checkout")
        .clone();
    let save = crate::cache_steps::tools_cache_step(
        false,
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION,
        Some(crate::cache_p08::tools_cache_save_condition()),
    )
    .expect("save");
    jobs.get_mut("rust-0__hosted")
        .expect("hosted")
        .steps
        .push(save);
    let shared = share_lanes(&jobs, &ctx()).expect("share");
    let hosted = shared.jobs.get("rust-0__hosted").expect("hosted");
    let local = shared.jobs.get("rust-0__local").expect("local");
    assert_eq!(
        shared.checkouts.get("rust-0__hosted"),
        Some(&expected_checkout)
    );
    assert_eq!(
        shared.checkouts.get("rust-0__local"),
        Some(&expected_checkout)
    );
    assert_eq!(hosted.steps.len(), 1);
    assert_eq!(hosted.steps.first().expect("save").name, "Save Mise tools");
    assert_eq!(local.steps, [] as [velnor_actions_contract::Step; 0]);
    let action = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/rust-0/action.yml")
        .expect("composite");
    assert!(!action.bytes.contains("Save Mise tools"));
    let yaml = render_jobs(&workflow_ir(), &shared, &ctx()).expect("yaml");
    assert_eq!(yaml.matches("Save Mise tools").count(), 1);
    let checkout_at = yaml.find("name: Checkout").expect("checkout");
    let call_at = yaml.find("uses: ./.github/actions/rust-0").expect("call");
    let save_at = yaml.find("name: Save Mise tools").expect("save");
    assert!(checkout_at < call_at && call_at < save_at);
}
