use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, SCALE_SET_NAME, ScaleSetSelector, Step, StepKind,
    Trigger, VELNOR_LABEL, WorkflowIr,
};

use super::workflow_to_yaml;
use crate::render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext};

fn runner_token() -> String {
    ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .expect("valid scale set")
    .token()
}

fn job(runs_on: &str) -> Job {
    Job {
        display_name: "Probe".to_owned(),
        runs_on: runs_on.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![Step {
            name: "Run probe".to_owned(),
            condition: None,
            kind: StepKind::Shell {
                run: vec!["echo".to_owned(), "probe".to_owned()],
                env: BTreeMap::new(),
            },
        }],
    }
}

fn context() -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        plan_consumer_env: BTreeMap::new(),
    }
}

fn workflow(jobs: BTreeMap<String, Job>) -> WorkflowIr {
    WorkflowIr {
        name: "Paired probe".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
    }
}

#[test]
fn typed_scale_set_jobs_declare_bash_while_hosted_jobs_keep_default() {
    let jobs = BTreeMap::from([
        ("hosted".to_owned(), job("ubuntu-26.04")),
        ("scale".to_owned(), job(&runner_token())),
    ]);
    let rendered = workflow_to_yaml(
        &workflow(jobs.clone()),
        &jobs,
        &context(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    )
    .expect("workflow renders");
    let yaml = crate::yaml::render_yaml(&rendered);
    assert!(
        yaml.contains("- name: Run probe\n        run: echo probe"),
        "generated shell steps must put name first for shell scanning: {yaml}"
    );
    let hosted = yaml.split("  hosted:\n").nth(1).expect("hosted job");
    let hosted = hosted.split("  scale:\n").next().expect("hosted boundary");
    let scale = yaml.split("  scale:\n").nth(1).expect("scale job");
    assert!(!hosted.contains("defaults:"), "{hosted}");
    assert!(
        scale.contains("defaults:\n      run:\n        shell: bash -e {0}"),
        "{scale}"
    );
}
