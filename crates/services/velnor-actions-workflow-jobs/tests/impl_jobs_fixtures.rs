//! Shared fixtures for the jobs test family (duplicated per test target).

use std::collections::BTreeMap;
use velnor_actions_contract_workflow::workflow::permissions::PermissionLevel;
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, Permissions, Step, Trigger, WorkflowIr,
};
use velnor_actions_workflow_jobs::context::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, FINAL_JOB_ID};
use velnor_actions_workflow_steps::{RenderError, shell_step};

pub(crate) const VERSION: &str = "0.1.0";
pub(crate) const LABEL: &str = "ubuntu-26.04";

pub(crate) fn checkout_pin() -> String {
    format!("actions/checkout@{:040x}", 0)
}

pub(crate) fn mise_argv(tool: &str, program: &str, extra: &[&str]) -> Vec<String> {
    let mut argv = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        tool.to_owned(),
        "--".to_owned(),
        program.to_owned(),
    ];
    argv.extend(extra.iter().map(ToString::to_string));
    argv
}

pub(crate) fn job(id: &str, display: &str, needs: Vec<String>, steps: Vec<Step>) -> (String, Job) {
    (
        id.to_owned(),
        Job {
            display_name: display.to_owned(),
            runs_on: LABEL.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::CRATE,
            outputs: Vec::new(),
            needs,
            condition: None,
            permissions: (id == FINAL_JOB_ID).then_some(Permissions {
                contents: PermissionLevel::Read,
                actions: PermissionLevel::Read,
                pull_requests: PermissionLevel::None,
                id_token: PermissionLevel::None,
            }),
            environment: None,
            steps,
        },
    )
}

pub(crate) fn fixture_ir(jobs: Vec<(String, Job)>) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
                .iter()
                .map(ToString::to_string)
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
        jobs: jobs.into_iter().collect(),
    }
}

pub(crate) fn scrubbed_shell_step(name: &str, argv: Vec<String>) -> Result<Step, RenderError> {
    shell_step(name, argv, BTreeMap::new())
}
