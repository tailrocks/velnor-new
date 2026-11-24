// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use crate::image::{Events, ImageWorkflow};
use crate::image_build::{only_live, sha_scope, stands_down};
use crate::install_steps::install_helper;
use crate::release::{ENVIRONMENT as RELEASE_ENVIRONMENT, HOSTED_RUNNER};
use crate::release_binaries::command;
use crate::release_steps::{
    APP_TOKEN_OUTPUT, app_token, expression, permissions, shell_word, variables,
};
use crate::rules::{RenderedConcurrency, RenderedJob, RenderedStep, Value};

const GATE_WAIT_SECONDS: u32 = 2400;
const GATE_TIMEOUT_MINUTES: u16 = 50;
const EXEMPTIONS_TIMEOUT_MINUTES: u16 = 10;
pub(crate) const GATE_STEP: &str = "gate";

pub(crate) fn gate_scopes(pr_only: &[String]) -> Vec<(String, String)> {
    let mut scopes = vec![
        ("actions", "read"),
        ("contents", "read"),
        ("statuses", "read"),
    ];
    if !pr_only.is_empty() {
        scopes.insert(1, ("checks", "read"));
    }
    permissions(&scopes)
}

pub(crate) struct GateOptions<'a> {
    pub(crate) wait: u32,
    pub(crate) tip: bool,
    pub(crate) pr_only: &'a [String],
    pub(crate) verify: bool,
    pub(crate) ignore: &'a [String],
    pub(crate) published: Option<&'a str>,
}

pub(crate) fn gate_step(options: &GateOptions<'_>) -> RenderedStep {
    let GateOptions {
        wait,
        tip,
        pr_only,
        verify,
        ignore,
        published,
    } = options;
    let wait = *wait;
    let mut flags = if *tip {
        " --require-tip".to_owned()
    } else {
        String::new()
    };
    if *verify {
        flags.push_str(" --verify-exemptions");
    }
    for name in *pr_only {
        let _ = write!(flags, " --pr-only-check={}", shell_word(name));
    }
    for name in *ignore {
        let _ = write!(flags, " --ignore-job={}", shell_word(name));
    }
    if let Some(job) = published {
        let _ = write!(flags, " --require-published={}", shell_word(job));
    }
    RenderedStep {
        id: Some(GATE_STEP.to_owned()),
        name: "Wait for every required check of the commit".to_owned(),
        env: variables(&[
            ("GITHUB_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("COMMIT", "${{ github.sha }}"),
        ]),
        run: Some(format!(
            "velnor-ci image gate --repo=\"$REPOSITORY\" --sha=\"$COMMIT\" --wait-seconds={wait}{flags}"
        )),
        ..RenderedStep::default()
    }
}

pub(crate) const EXEMPTIONS_JOB: &str = "image-exemptions";
const EXEMPTIONS_STEP: &str = "exemptions";

pub(crate) fn admin_token(
    workflow: &ImageWorkflow<'_>,
    mut job: RenderedJob,
    position: usize,
) -> RenderedJob {
    let scopes = [
        ("actions", "read"),
        ("checks", "read"),
        ("contents", "read"),
        ("statuses", "read"),
        ("administration", "write"),
    ];
    job.steps
        .insert(position, app_token(workflow.helper, &scopes));
    for step in &mut job.steps {
        if matches!(step.id.as_deref(), Some(GATE_STEP | EXEMPTIONS_STEP)) {
            step.env.retain(|(name, _)| name != "GITHUB_TOKEN");
            step.env.insert(
                0,
                (
                    "GITHUB_TOKEN".to_owned(),
                    Value::Expr(APP_TOKEN_OUTPUT.to_owned()),
                ),
            );
        }
    }
    HOSTED_RUNNER.clone_into(&mut job.runs_on);
    job.environment = Some(RELEASE_ENVIRONMENT.to_owned());
    job
}

pub(crate) fn exemptions_job(workflow: &ImageWorkflow<'_>, events: &Events<'_>) -> RenderedJob {
    let pr_only = workflow.pr_only_checks();
    let mut flags = String::new();
    for name in &pr_only {
        let _ = write!(flags, " --pr-only-check={}", shell_word(name));
    }
    RenderedJob {
        id: EXEMPTIONS_JOB.to_owned(),
        condition: Some(expression(&events.branch_push())),
        permissions: gate_scopes(&pr_only),
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: EXEMPTIONS_TIMEOUT_MINUTES,
        steps: vec![
            install_helper(workflow.helper),
            RenderedStep {
                id: Some(EXEMPTIONS_STEP.to_owned()),
                env: variables(&[
                    ("GITHUB_TOKEN", "${{ github.token }}"),
                    ("REPOSITORY", "${{ github.repository }}"),
                    ("COMMIT", "${{ github.sha }}"),
                ]),
                ..command(
                    "Verify that the PR-only checks sit under a non-bypassable ruleset".to_owned(),
                    format!(
                        "velnor-ci image gate --repo=\"$REPOSITORY\" --sha=\"$COMMIT\" --only=exemptions{flags}"
                    ),
                )
            },
        ],
        ..RenderedJob::default()
    }
}

pub(crate) const GATE_JOB: &str = "image-gate";

pub(crate) fn gate_ignored(workflow: &ImageWorkflow<'_>) -> Vec<String> {
    let mut jobs = vec![GATE_JOB.to_owned()];
    for build in &workflow.images {
        jobs.push(crate::image_build::job_id(build.image, "publish"));
    }
    jobs
}

pub(crate) fn gate_job(workflow: &ImageWorkflow<'_>, events: &Events<'_>) -> RenderedJob {
    RenderedJob {
        concurrency: Some(RenderedConcurrency {
            group: format!("{GATE_JOB}-{}", sha_scope()),
            cancel_in_progress: true,
        }),
        id: GATE_JOB.to_owned(),
        condition: Some(expression(&events.branch_push())),
        needs: if workflow.pr_only_checks().is_empty() {
            Vec::new()
        } else {
            vec![EXEMPTIONS_JOB.to_owned()]
        },
        permissions: gate_scopes(&workflow.pr_only_checks()),
        outputs: vec![(
            "ready".to_owned(),
            format!("${{{{ steps.{GATE_STEP}.outputs.ready }}}}"),
        )],
        runs_on: workflow.control_runner(),
        timeout_minutes: GATE_TIMEOUT_MINUTES,
        steps: vec![
            install_helper(workflow.helper),
            stands_down(),
            only_live(gate_step(&GateOptions {
                wait: GATE_WAIT_SECONDS,
                tip: false,
                pr_only: &workflow.pr_only_checks(),
                verify: false,
                ignore: &gate_ignored(workflow),
                published: None,
            })),
        ],
        ..RenderedJob::default()
    }
}
