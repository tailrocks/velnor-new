// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use crate::image::{IMAGE_ENVIRONMENT, ImageBuild, ImageWorkflow};
use crate::image_build::{job_id, setup_buildx};
use crate::image_publish::{DIGEST_OUTPUT, host_guard, login, registry_scopes};
use crate::install_steps::install_helper;
use crate::release_binaries::command;
use crate::release_steps::{expression, shell_word, variables};
use crate::rules::{RenderedConcurrency, RenderedJob, RenderedStep, Value};

const ADVANCE_TIMEOUT_MINUTES: u16 = 10;
const ADVANCE_STEP: &str = "advance";

fn withheld(branch: &str) -> Vec<RenderedStep> {
    let not_moved = expression(&format!("steps.{ADVANCE_STEP}.outputs.moved != 'true'"));
    let notice = format!(
        "echo \"::notice title=Image::the {branch} tag was not moved by this commit because the tip has no image to point it at\""
    );
    let summary = format!(
        "echo \"The {branch} tag was not moved by this run: the tip of {branch} has no image in every registry, so only sha-$COMMIT was pushed.\" >> \"$GITHUB_STEP_SUMMARY\""
    );
    vec![
        RenderedStep {
            condition: Some(not_moved.clone()),
            ..command(
                "Say that the default-branch tag was withheld".to_owned(),
                notice,
            )
        },
        RenderedStep {
            condition: Some(not_moved),
            env: variables(&[("COMMIT", "${{ github.sha }}")]),
            ..command(
                "Record that the default-branch tag was withheld".to_owned(),
                summary,
            )
        },
    ]
}

fn advance_step(workflow: &ImageWorkflow<'_>, build: &ImageBuild<'_>) -> RenderedStep {
    let image = build.image;
    let publish = job_id(image, "publish");
    let mut images = String::new();
    for registry in &image.registries {
        let _ = write!(images, " --image={}", shell_word(&registry.reference()));
    }
    RenderedStep {
        id: Some(ADVANCE_STEP.to_owned()),
        env: vec![
            (
                "GITHUB_TOKEN".to_owned(),
                Value::Expr("${{ github.token }}".to_owned()),
            ),
            (
                "REPOSITORY".to_owned(),
                Value::Expr("${{ github.repository }}".to_owned()),
            ),
            (
                "COMMIT".to_owned(),
                Value::Expr("${{ github.sha }}".to_owned()),
            ),
            ("BRANCH".to_owned(), Value::Text(workflow.branch.to_owned())),
            (
                "DIGEST".to_owned(),
                Value::Expr(format!(
                    "${{{{ needs.{publish}.outputs.{DIGEST_OUTPUT} }}}}"
                )),
            ),
        ],
        ..command(
            "Point the default-branch tag at the image of the tip".to_owned(),
            format!(
                "velnor-ci image advance --repo \"$REPOSITORY\" --commit \"$COMMIT\" --digest \"$DIGEST\" --branch \"$BRANCH\"{images}"
            ),
        )
    }
}

pub(crate) fn advance_job(workflow: &ImageWorkflow<'_>, build: &ImageBuild<'_>) -> RenderedJob {
    let image = build.image;
    let publish = job_id(image, "publish");
    let mut steps = vec![install_helper(workflow.helper), setup_buildx()];
    steps.extend(host_guard(image));
    steps.extend(image.registries.iter().map(login));
    steps.push(advance_step(workflow, build));
    steps.extend(withheld(workflow.branch));
    RenderedJob {
        concurrency: Some(RenderedConcurrency {
            group: format!("{}-${{{{ github.ref }}}}", job_id(image, "advance")),
            cancel_in_progress: false,
        }),
        id: job_id(image, "advance"),
        needs: vec![publish],
        environment: Some(IMAGE_ENVIRONMENT.to_owned()),
        permissions: registry_scopes(image),
        runs_on: workflow.publish_runner(build),
        timeout_minutes: ADVANCE_TIMEOUT_MINUTES,
        steps,
        ..RenderedJob::default()
    }
}
