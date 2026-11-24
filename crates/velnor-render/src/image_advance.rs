// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::image::{IMAGE_ENVIRONMENT, ImageBuild, ImageWorkflow};
use crate::image_build::{job_id, setup_buildx};
use crate::image_publish::{DIGEST_OUTPUT, TIP_STEP, login, registry_scopes, tip};
use crate::install_steps::install_helper;
use crate::release_binaries::command;
use crate::release_steps::{expression, variables};
use crate::rules::{RenderedConcurrency, RenderedJob, RenderedStep, Value};

const ADVANCE_TIMEOUT_MINUTES: u16 = 10;

fn only(condition: &str, mut step: RenderedStep) -> RenderedStep {
    step.condition = Some(expression(condition));
    step
}

fn withheld(branch: &str) -> Vec<RenderedStep> {
    let not_tip = format!("steps.{TIP_STEP}.outputs.ready != 'true'");
    let notice = format!(
        "echo \"::notice title=Image::the {branch} tag was withheld because this commit is no longer the tip\""
    );
    let summary = format!(
        "echo \"The {branch} tag was withheld: this commit is no longer the tip of {branch}, so only sha-$COMMIT was pushed.\" >> \"$GITHUB_STEP_SUMMARY\""
    );
    vec![
        only(
            &not_tip,
            command(
                "Say that the default-branch tag was withheld".to_owned(),
                notice,
            ),
        ),
        only(
            &not_tip,
            RenderedStep {
                env: variables(&[("COMMIT", "${{ github.sha }}")]),
                ..command(
                    "Record that the default-branch tag was withheld".to_owned(),
                    summary,
                )
            },
        ),
    ]
}

pub(crate) fn advance_job(workflow: &ImageWorkflow<'_>, build: &ImageBuild<'_>) -> RenderedJob {
    let image = build.image;
    let ready = format!("steps.{TIP_STEP}.outputs.ready == 'true'");
    let publish = job_id(image, "publish");
    let mut steps = vec![install_helper(workflow.helper), tip(TIP_STEP, None)];
    steps.push(only(&ready, setup_buildx()));
    steps.extend(
        image
            .registries
            .iter()
            .map(|registry| only(&ready, login(registry))),
    );
    for registry in &image.registries {
        steps.push(only(
            &ready,
            RenderedStep {
                env: vec![
                    ("IMAGE".to_owned(), Value::Text(registry.reference())),
                    ("BRANCH".to_owned(), Value::Text(workflow.branch.to_owned())),
                    (
                        "DIGEST".to_owned(),
                        Value::Expr(format!(
                            "${{{{ needs.{publish}.outputs.{DIGEST_OUTPUT} }}}}"
                        )),
                    ),
                ],
                ..command(
                    format!(
                        "Move {} to the pushed digest in {}",
                        workflow.branch,
                        registry.host.name()
                    ),
                    "docker buildx imagetools create --tag \"$IMAGE:$BRANCH\" \"$IMAGE@$DIGEST\""
                        .to_owned(),
                )
            },
        ));
    }
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
