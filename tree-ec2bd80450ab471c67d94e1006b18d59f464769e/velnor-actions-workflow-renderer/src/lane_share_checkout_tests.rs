use super::*;

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
fn matching_lane_checkout_inputs_stay_outside_the_composite() {
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
