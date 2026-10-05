use std::collections::BTreeMap;

use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, SCALE_SET_NAME, ScaleSetSelector, Step, StepKind,
    Trigger, VELNOR_LABEL, WorkflowIr,
};

use super::{HOSTED_SUFFIX, SCALE_SUFFIX, share_lanes};
use crate::RenderError;
use crate::render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext};

const HOSTED_RUNS: &str = "ubuntu-26.04";
const CAP: usize = 500_000;
const LOGICAL_JOBS: usize = 21;
const STEPS_PER_JOB: usize = 48;

<<<<<<< HEAD
fn ctx() -> RenderContext {
=======
pub(super) fn ctx() -> RenderContext {
>>>>>>> origin/main
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: HOSTED_RUNS.to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        plan_consumer_env: BTreeMap::new(),
    }
}

<<<<<<< HEAD
fn workflow_ir() -> WorkflowIr {
=======
pub(super) fn workflow_ir() -> WorkflowIr {
>>>>>>> origin/main
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

<<<<<<< HEAD
fn echo_step(index: usize, payload: &str) -> Step {
=======
pub(super) fn echo_step(index: usize, payload: &str) -> Step {
>>>>>>> origin/main
    Step {
        name: format!("echo {index}"),
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
        steps,
    }
}

<<<<<<< HEAD
fn render_jobs(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    calls: &BTreeMap<String, Vec<super::SharedCall>>,
=======
fn checkout() -> Step {
    Step {
        name: "Checkout".to_owned(),
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
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    calls: &BTreeMap<String, String>,
    checkouts: &BTreeMap<String, Step>,
>>>>>>> origin/main
) -> Result<String, crate::RenderError> {
    let document = crate::document::workflow_to_yaml(
        ir,
        jobs,
        ctx,
        calls,
<<<<<<< HEAD
=======
        checkouts,
>>>>>>> origin/main
        &std::collections::BTreeSet::new(),
    )?;
    let quoted = crate::yaml::quote_run_values_in_yaml(document);
    crate::marker::with_marker(&ctx.generator_version, &crate::yaml::render_yaml(&quoted))
}

<<<<<<< HEAD
=======
fn positions(yaml: &str, needle: &str) -> Vec<usize> {
    yaml.match_indices(needle)
        .map(|(position, _)| position)
        .collect()
}

>>>>>>> origin/main
fn heavy_steps(payload: &str) -> Vec<Step> {
    (0..STEPS_PER_JOB)
        .map(|index| echo_step(index, payload))
        .collect()
}

<<<<<<< HEAD
fn paired(steps: &[Step]) -> BTreeMap<String, Job> {
    let scale = scale_token();
=======
pub(super) fn paired(steps: &[Step]) -> BTreeMap<String, Job> {
    let scale = scale_token();
    let mut lane_steps = vec![checkout()];
    lane_steps.extend_from_slice(steps);
>>>>>>> origin/main
    let mut jobs = BTreeMap::new();
    for index in 0..LOGICAL_JOBS {
        let logical = format!("rust-{index}");
        jobs.insert(
            format!("{logical}{HOSTED_SUFFIX}"),
<<<<<<< HEAD
            lane_job(&format!("{logical} hosted"), HOSTED_RUNS, steps.to_vec()),
        );
        jobs.insert(
            format!("{logical}{SCALE_SUFFIX}"),
            lane_job(&format!("{logical} local"), &scale, steps.to_vec()),
=======
            lane_job(
                &format!("{logical} hosted"),
                HOSTED_RUNS,
                lane_steps.clone(),
            ),
        );
        jobs.insert(
            format!("{logical}{SCALE_SUFFIX}"),
            lane_job(&format!("{logical} local"), &scale, lane_steps.clone()),
>>>>>>> origin/main
        );
    }
    jobs
}

#[test]
<<<<<<< HEAD
=======
fn shared_lanes_keep_ci_under_github_file_cap() {
    let payload = "a".repeat(400);
    let jobs = paired(&heavy_steps(&payload));
    let ir = workflow_ir();
    let context = ctx();
    let unshared =
        render_jobs(&ir, &jobs, &context, &BTreeMap::new(), &BTreeMap::new()).expect("unshared");
    assert!(
        unshared.len() > CAP,
        "unshared render must exceed the GitHub cap, got {}",
        unshared.len()
    );
    let shared = share_lanes(&jobs, &context).expect("share");
    let yaml = render_jobs(
        &ir,
        &shared.jobs,
        &context,
        &shared.calls,
        &shared.checkouts,
    )
    .expect("shared");
    assert!(
        yaml.len() <= CAP,
        "shared ci.yml must fit, got {}",
        yaml.len()
    );
    assert!(
        !yaml.contains(&payload),
        "shared ci.yml still inlines steps"
    );
    assert!(yaml.contains("runs-on: ubuntu-26.04"));
    assert!(yaml.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"));
    assert!(yaml.contains("uses: ./.github/actions/rust-0"));
    let checkouts = positions(&yaml, "uses: actions/checkout@");
    let calls = positions(&yaml, "uses: ./.github/actions/rust-");
    assert_eq!(checkouts.len(), LOGICAL_JOBS * 2);
    assert_eq!(calls.len(), LOGICAL_JOBS * 2);
    assert_eq!(
        yaml.matches("# zizmor: ignore[self-repository]").count(),
        LOGICAL_JOBS * 2
    );
    for (index, (checkout, call)) in checkouts.iter().zip(&calls).enumerate() {
        assert!(
            checkout < call,
            "job {index} calls shared action before checkout"
        );
        if let Some(next_checkout) = checkouts.get(index + 1) {
            assert!(call < next_checkout, "shared job sequence interleaved");
        }
    }
    assert_eq!(shared.files.len(), LOGICAL_JOBS);
    for file in &shared.files {
        assert!(
            file.bytes.len() <= CAP,
            "{} is {} bytes",
            file.path,
            file.bytes.len()
        );
        assert!(file.bytes.contains("shell: bash"), "{}", file.path);
        assert!(file.bytes.contains(&payload), "{}", file.path);
        assert!(file.path.ends_with("/action.yml"), "{}", file.path);
    }
}

#[test]
>>>>>>> origin/main
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

<<<<<<< HEAD
#[test]
fn elected_save_stays_on_the_winner_job() {
    let mut jobs = paired(&[echo_step(0, "one")]);
=======
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
    let yaml = render_jobs(
        &workflow_ir(),
        &shared.jobs,
        &ctx(),
        &shared.calls,
        &shared.checkouts,
    )
    .expect("yaml");
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
    assert!(shared.files.is_empty());
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
>>>>>>> origin/main
    let save = crate::cache_steps::tools_save_step("mise-v1").expect("save");
    jobs.get_mut("rust-0__hosted")
        .expect("hosted")
        .steps
        .push(save);
    let shared = share_lanes(&jobs, &ctx()).expect("share");
    let hosted = shared.jobs.get("rust-0__hosted").expect("hosted");
    let local = shared.jobs.get("rust-0__local").expect("local");
<<<<<<< HEAD
    assert_eq!(hosted.steps.len(), 1);
    assert_eq!(hosted.steps.first().expect("save").name, "Save Mise tools");
    assert_eq!(local.steps.len(), 0);
=======
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
    assert!(local.steps.is_empty());
>>>>>>> origin/main
    let action = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/rust-0/action.yml")
        .expect("composite");
    assert!(!action.bytes.contains("Save Mise tools"));
<<<<<<< HEAD
    let yaml = render_jobs(&workflow_ir(), &shared.jobs, &ctx(), &shared.calls).expect("yaml");
    assert_eq!(yaml.matches("Save Mise tools").count(), 1);
}

fn mbx_restore() -> Step {
    crate::cache_steps::mbx_objects_step(
        &format!("jdx/mr-boxington-action@{:040x}", 0),
        false,
        "1.22.0",
    )
    .expect("MBX restore")
}

fn job_body<'a>(yaml: &'a str, id: &str) -> &'a str {
    let marker = format!("  {id}:");
    let body = yaml.split_once(&marker).expect("job marker").1;
    let body = body.split_once('\n').map_or(body, |(_, body)| body);
    let mut offset = 0;
    while let Some(relative) = body[offset..].find("\n  ") {
        let end = offset + relative;
        if !body[end + 3..].starts_with(' ') {
            return &body[..end];
        }
        offset = end + 3;
    }
    body
}

#[test]
fn mbx_both_mode_shared_segments_preserve_order_and_fit_github_file_cap() {
    let payload = "a".repeat(400);
    let jobs = mbx_stress_jobs(&heavy_steps(&payload));
    let ir = workflow_ir();
    let context = ctx();
    let unshared = render_jobs(&ir, &jobs, &context, &BTreeMap::new()).expect("unshared");
    assert!(
        unshared.len() > CAP,
        "unshared MBX both-mode render must exceed the GitHub cap, got {}",
        unshared.len()
    );

    let mut shared = share_lanes(&jobs, &context).expect("share");
    assert_mbx_stress_call_offsets(&shared);
    crate::cache_steps::isolate_hosted_mbx_object_caches(&mut shared.jobs);
    let yaml = render_jobs(&ir, &shared.jobs, &context, &shared.calls).expect("shared");
    assert!(
        yaml.len() <= CAP,
        "shared both-mode ci.yml must fit, got {}",
        yaml.len()
    );
    assert!(
        !yaml.contains(&payload),
        "shared ci.yml still inlines steps"
    );
    assert_mbx_stress_yaml_order(&yaml);
}

fn mbx_stress_jobs(steps: &[Step]) -> BTreeMap<String, Job> {
    let mut jobs = paired(steps);
    for index in 0..LOGICAL_JOBS {
        jobs.get_mut(&format!("rust-{index}__hosted"))
            .expect("hosted")
            .steps
            .insert(1, mbx_restore());
        let local = jobs
            .get_mut(&format!("rust-{index}__local"))
            .expect("local");
        if index == 0 {
            local.steps.insert(1, echo_step(48, "lane-only"));
        }
        local.steps.insert(2, mbx_restore());
    }
    jobs.get_mut("rust-0__hosted")
        .expect("hosted")
        .steps
        .push(crate::cache_steps::tools_save_step("mise-v1").expect("save"));
    jobs
}

fn assert_mbx_stress_call_offsets(shared: &super::LaneShare) {
    assert_eq!(shared.files.len(), LOGICAL_JOBS * 3 - 1);
    for index in 0..LOGICAL_JOBS {
        let logical = format!("rust-{index}");
        let hosted_calls = &shared.calls[&format!("{logical}{HOSTED_SUFFIX}")];
        let local_calls = &shared.calls[&format!("{logical}{SCALE_SUFFIX}")];
        assert_eq!(
            hosted_calls
                .iter()
                .map(|call| (call.before_step, call.skip_steps))
                .collect::<Vec<_>>(),
            if index == 0 {
                vec![(0, 1), (2, 47)]
            } else {
                vec![(0, 1), (2, 1), (3, 46)]
            }
        );
        assert_eq!(
            local_calls
                .iter()
                .map(|call| (call.before_step, call.skip_steps))
                .collect::<Vec<_>>(),
            if index == 0 {
                vec![(0, 1), (3, 47)]
            } else {
                vec![(0, 1), (1, 1), (3, 46)]
            }
        );
    }
    assert!(
        shared
            .files
            .iter()
            .all(|file| !file.bytes.contains("Restore MBX objects"))
    );
    assert!(
        shared
            .files
            .iter()
            .all(|file| !file.bytes.contains("lane-only")),
        "lane-only step must stay in the local job"
    );
}

fn assert_mbx_stress_yaml_order(yaml: &str) {
    assert!(yaml.contains("run: echo lane-only"));
    assert_eq!(
        yaml.matches("name: Restore MBX objects").count(),
        LOGICAL_JOBS * 2
    );
    assert_eq!(
        yaml.matches("isolate-objects-cache: \"true\"").count(),
        LOGICAL_JOBS
    );
    assert_eq!(
        yaml.matches("cache-key-suffix: ${{ github.job }}").count(),
        LOGICAL_JOBS
    );
    assert_eq!(yaml.matches("Save Mise tools").count(), 1);

    let hosted = job_body(yaml, "rust-0__hosted");
    let local = job_body(yaml, "rust-0__local");
    let hosted_calls: Vec<usize> = hosted
        .match_indices("name: Run shared steps")
        .map(|(at, _)| at)
        .collect();
    let local_calls: Vec<usize> = local
        .match_indices("name: Run shared steps")
        .map(|(at, _)| at)
        .collect();
    let restore = hosted.find("name: Restore MBX objects").expect("restore");
    let save = hosted.find("name: Save Mise tools").expect("save");
    let local_restore = local
        .find("name: Restore MBX objects")
        .expect("local restore");
    let lane_only = local.find("name: echo 48").expect("lane-only step");
    assert_eq!(hosted_calls.len(), 2);
    assert_eq!(local_calls.len(), 2);
    assert!(
        hosted_calls[0] < restore && restore < hosted_calls[1],
        "{hosted}"
    );
    assert!(hosted_calls[1] < save, "{hosted}");
    assert!(
        local_calls[0] < lane_only && lane_only < local_restore && local_restore < local_calls[1],
        "{local}"
    );
    assert!(
        hosted.contains("isolate-objects-cache: \"true\""),
        "{hosted}"
    );
    assert!(!local.contains("isolate-objects-cache"), "{local}");
    assert!(!local.contains("cache-key-suffix"), "{local}");
    assert!(local.contains("name: Restore MBX objects"), "{local}");
    assert!(!local.contains("Save Mise tools"), "{local}");
}

#[test]
fn mbx_segment_paths_do_not_collide_with_ordinary_logical_ids() {
    let before = echo_step(0, "before restore");
    let after = echo_step(1, "after restore");
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "rust-a__hosted".to_owned(),
        lane_job(
            "hosted MBX",
            HOSTED_RUNS,
            vec![before.clone(), mbx_restore(), after.clone()],
        ),
    );
    jobs.insert(
        "rust-a__local".to_owned(),
        lane_job(
            "local MBX",
            &scale_token(),
            vec![before, mbx_restore(), after],
        ),
    );
    jobs.insert(
        "rust-a-shared-0__hosted".to_owned(),
        lane_job(
            "hosted ordinary",
            HOSTED_RUNS,
            vec![echo_step(2, "ordinary")],
        ),
    );
    jobs.insert(
        "rust-a-shared-0__local".to_owned(),
        lane_job(
            "local ordinary",
            &scale_token(),
            vec![echo_step(2, "ordinary")],
        ),
    );

    let shared = share_lanes(&jobs, &ctx()).expect("share");
    let paths: std::collections::BTreeSet<_> =
        shared.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths.len(), shared.files.len(), "duplicate composite paths");
    assert!(paths.contains(".github/actions/shared-lanes/rust-a/segment-0/action.yml"));
    assert!(paths.contains(".github/actions/shared-lanes/rust-a/segment-1/action.yml"));
    assert!(paths.contains(".github/actions/rust-a-shared-0/action.yml"));
}

#[path = "lane_share_lcs_tests.rs"]
mod lcs;
=======
    let yaml = render_jobs(
        &workflow_ir(),
        &shared.jobs,
        &ctx(),
        &shared.calls,
        &shared.checkouts,
    )
    .expect("yaml");
    assert_eq!(yaml.matches("Save Mise tools").count(), 1);
    let checkout_at = yaml.find("name: Checkout").expect("checkout");
    let call_at = yaml.find("uses: ./.github/actions/rust-0").expect("call");
    let save_at = yaml.find("name: Save Mise tools").expect("save");
    assert!(checkout_at < call_at && call_at < save_at);
}
>>>>>>> origin/main
