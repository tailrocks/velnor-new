use super::{
    HOSTED_RUNS, checkout, ctx, echo_step, lane_job, paired, render_jobs, scale_token, share_lanes,
    workflow_ir,
};
use std::collections::BTreeMap;
use velnor_actions_contract_workflow::{Job, StepKind};
use velnor_actions_workflow_steps::RenderError;

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
    let save =
        velnor_actions_workflow_cache::cache_steps::tools_save_step("mise-v1").expect("save");
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
    assert!(local.steps.is_empty());
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
