// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::image::{Events, IMAGE_ENVIRONMENT, ImageBuild, ImageWorkflow};
use crate::image_build::{job_id, setup_buildx};
use crate::image_publish::{gate_scopes, gate_step, login, registry_scopes};
use crate::install_steps::install_helper;
use crate::release_binaries::command;
use crate::release_steps::{expression, variables};
use crate::rules::{RenderedJob, RenderedStep, Value};

const PROMOTE_TIMEOUT_MINUTES: u16 = 10;
const VERSION_STEP: &str = "version";

pub(crate) fn authority_job(
    workflow: &ImageWorkflow<'_>,
    build: &ImageBuild<'_>,
    events: &Events<'_>,
) -> RenderedJob {
    let image = build.image;
    let strict = format!(
        "^{}-v[0-9]{{1,40}}[.][0-9]{{1,40}}[.][0-9]{{1,40}}$",
        image.name
    );
    let strip = format!("version=${{REF_NAME#{}-v}}", image.name);
    let ref_name = || variables(&[("REF_NAME", "${{ github.ref_name }}")]);
    RenderedJob {
        id: job_id(image, "authority"),
        condition: Some(expression(&events.tag_push(image))),
        permissions: gate_scopes(&workflow.pr_only_checks()),
        outputs: vec![(
            "version".to_owned(),
            format!("${{{{ steps.{VERSION_STEP}.outputs.version }}}}"),
        )],
        runs_on: workflow.runner.to_string(),
        timeout_minutes: PROMOTE_TIMEOUT_MINUTES,
        steps: vec![
            install_helper(workflow.helper),
            RenderedStep {
                env: ref_name(),
                ..command(
                    "Check that the tag names a strict version".to_owned(),
                    format!("[[ \"$REF_NAME\" =~ {strict} ]]"),
                )
            },
            gate_step(0, false, &workflow.pr_only_checks()),
            RenderedStep {
                id: Some(VERSION_STEP.to_owned()),
                env: ref_name(),
                ..command(
                    "Read the version from the tag".to_owned(),
                    format!("echo \"{strip}\" >> \"$GITHUB_OUTPUT\""),
                )
            },
        ],
        ..RenderedJob::default()
    }
}

pub(crate) fn promote_job(workflow: &ImageWorkflow<'_>, build: &ImageBuild<'_>) -> RenderedJob {
    let image = build.image;
    let mut steps = vec![setup_buildx()];
    steps.extend(image.registries.iter().map(login));
    for registry in &image.registries {
        steps.push(RenderedStep {
            env: vec![
                ("IMAGE".to_owned(), Value::Text(registry.reference())),
                (
                    "VERSION".to_owned(),
                    Value::Expr(format!(
                        "${{{{ needs.{}.outputs.version }}}}",
                        job_id(image, "authority")
                    )),
                ),
                (
                    "COMMIT".to_owned(),
                    Value::Expr("${{ github.sha }}".to_owned()),
                ),
            ],
            ..command(
                format!("Promote the pushed digest in {}", registry.host.name()),
                "docker buildx imagetools create --tag \"$IMAGE:$VERSION\" \"$IMAGE:sha-$COMMIT\""
                    .to_owned(),
            )
        });
    }
    RenderedJob {
        id: job_id(image, "promote"),
        needs: vec![job_id(image, "authority")],
        environment: Some(IMAGE_ENVIRONMENT.to_owned()),
        permissions: registry_scopes(image),
        runs_on: workflow.runner.to_string(),
        timeout_minutes: PROMOTE_TIMEOUT_MINUTES,
        steps,
        ..RenderedJob::default()
    }
}
