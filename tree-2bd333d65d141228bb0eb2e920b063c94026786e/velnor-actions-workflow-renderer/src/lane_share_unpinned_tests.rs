use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, Step, StepKind, StepRole};

use super::{HOSTED_SUFFIX, SCALE_SUFFIX, share_lanes};
use crate::RenderError;
use crate::render::RenderContext;

fn context(checkout_uses: &str) -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: checkout_uses.to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        plan_consumer_env: BTreeMap::new(),
    }
}

fn step(name: &str, uses: &str) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

fn checkout_step(uses: &str) -> Step {
    Step {
        name: "Checkout".to_owned(),
        id: None,
        role: Some(StepRole::Checkout),
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]),
            env: BTreeMap::new(),
        },
    }
}

fn job(display_name: &str, runs_on: &str, steps: Vec<Step>) -> Job {
    Job {
        display_name: display_name.to_owned(),
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

fn paired_action(checkout_uses: &str, action_uses: &str) -> BTreeMap<String, Job> {
    let steps = vec![
        checkout_step(checkout_uses),
        step("Use remote action", action_uses),
    ];
    BTreeMap::from([
        (
            format!("rust-0{HOSTED_SUFFIX}"),
            job("hosted", "ubuntu-26.04", steps.clone()),
        ),
        (
            format!("rust-0{SCALE_SUFFIX}"),
            job("local", "ubuntu-26.04-scale-set", steps),
        ),
    ])
}

#[test]
fn shared_composite_rejects_unpinned_remote_action_refs() {
    let checkout = format!("actions/checkout@{:040x}", 0);
    for uses in ["actions/cache/restore@main", "actions/cache/restore@v6"] {
        let jobs = paired_action(&checkout, uses);
        let result = share_lanes(&jobs, &context(&checkout));
        assert!(
            matches!(result, Err(RenderError::BadActionRef(ref problem)) if problem == &format!("unpinned_ref:{uses}")),
            "shared step {uses} must stay subject to full-SHA validation: {result:?}"
        );
    }
}

#[test]
fn shared_composite_keeps_pinned_remote_refs() {
    let checkout = format!("actions/checkout@{:040x}", 0);
    let pinned = crate::cache_steps::TOOLS_RESTORE_ACTION_USES;
    let jobs = paired_action(&checkout, pinned);
    let shared = share_lanes(&jobs, &context(&checkout)).expect("pinned action is accepted");
    let composite = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/rust-0/action.yml")
        .expect("shared action exists");
    let yaml = &composite.bytes;
    assert!(yaml.contains(pinned), "pinned action retained: {yaml}");
}
