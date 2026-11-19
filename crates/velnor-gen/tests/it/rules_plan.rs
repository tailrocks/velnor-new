// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{
    Concurrency, Job, JobId, Permissions, PrRun, PullRequestType, RunnerLabel, Step, StepKind,
    Triggers, When, WorkflowPlan,
};

pub(crate) fn plan(kinds: Vec<StepKind>) -> Result<WorkflowPlan, Box<dyn Error>> {
    Ok(WorkflowPlan {
        name: "Rust CI".to_owned(),
        triggers: Triggers {
            pull_request: vec![PullRequestType::Opened],
            push_branch: None,
            merge_group: false,
            schedule: None,
            workflow_dispatch: false,
        },
        concurrency: Concurrency::CancelOlderPullRequestRuns,
        permissions: Permissions::default(),
        fork_runner: RunnerLabel::new("ubuntu-latest")?,
        hosted_runners: velnor_model::HostedRunners::new(vec![RunnerLabel::new("ubuntu-latest")?]),
        jobs: vec![Job {
            id: JobId::new("format")?,
            name: None,
            comment: None,
            needs: Vec::new(),
            when: When::Success,
            pr: PrRun::Always,
            runs_on: RunnerLabel::new("ubuntu-latest")?,
            permissions: Permissions::default(),
            timeout_minutes: 10,
            gate: None,
            plan_outputs: Vec::new(),
            group_outputs: Vec::new(),
            reports_ignored: false,
            steps: kinds
                .into_iter()
                .map(|kind| Step {
                    kind,
                    directory: None,
                    manifest: None,
                    target_dir: None,
                    when: When::Success,
                    skip_on_draft: false,
                    skip_after_merge_group: None,
                    pr: PrRun::Always,
                    gate: None,
                    reads: None,
                })
                .collect(),
        }],
    })
}
