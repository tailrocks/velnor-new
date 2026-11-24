// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::image::{Events, IMAGE_ENVIRONMENT, ImageBuild, ImageWorkflow};
use crate::image_build::{job_id, setup_buildx};
use crate::image_gate::{gate_scopes, gate_step};
use crate::image_publish::{login, registry_scopes};
use crate::install_steps::install_helper;
use crate::release_binaries::command;
use crate::release_steps::{expression, variables};
use crate::rules::{RenderedConcurrency, RenderedJob, RenderedStep, Value};
use velnor_model::ImageRegistry;

const PROMOTE_TIMEOUT_MINUTES: u16 = 10;
const AUTHORITY_WAIT_SECONDS: u32 = 1200;
const AUTHORITY_WAIT_MINUTES: u16 = 30;
const VERSION_STEP: &str = "version";

pub(crate) fn authority_job(
    workflow: &ImageWorkflow<'_>,
    build: &ImageBuild<'_>,
    events: &Events<'_>,
) -> RenderedJob {
    let image = build.image;
    let pr_only = workflow.pr_only_checks();
    let strict = format!(
        "^{}-v[0-9]{{1,40}}[.][0-9]{{1,40}}[.][0-9]{{1,40}}$",
        image.name
    );
    let strip = format!("version=${{REF_NAME#{}-v}}", image.name);
    let ref_name = || variables(&[("REF_NAME", "${{ github.ref_name }}")]);
    RenderedJob {
        id: job_id(image, "authority"),
        condition: Some(expression(&events.tag_push(image))),
        permissions: gate_scopes(&pr_only),
        outputs: vec![(
            "version".to_owned(),
            format!("${{{{ steps.{VERSION_STEP}.outputs.version }}}}"),
        )],
        runs_on: workflow.control_runner(),
        timeout_minutes: if pr_only.is_empty() {
            PROMOTE_TIMEOUT_MINUTES
        } else {
            AUTHORITY_WAIT_MINUTES
        },
        steps: vec![
            install_helper(workflow.helper),
            RenderedStep {
                env: ref_name(),
                ..command(
                    "Check that the tag names a strict version".to_owned(),
                    format!("[[ \"$REF_NAME\" =~ {strict} ]]"),
                )
            },
            gate_step(
                if pr_only.is_empty() {
                    0
                } else {
                    AUTHORITY_WAIT_SECONDS
                },
                false,
                &pr_only,
                !pr_only.is_empty(),
            ),
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
    for (index, registry) in image.registries.iter().enumerate() {
        steps.extend(exists(index, registry));
    }
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
        concurrency: Some(RenderedConcurrency {
            group: format!("{}-${{{{ github.ref }}}}", job_id(image, "promote")),
            cancel_in_progress: false,
        }),
        id: job_id(image, "promote"),
        needs: vec![job_id(image, "authority")],
        environment: Some(IMAGE_ENVIRONMENT.to_owned()),
        permissions: registry_scopes(image),
        runs_on: workflow.publish_runner(build),
        timeout_minutes: PROMOTE_TIMEOUT_MINUTES,
        steps,
        ..RenderedJob::default()
    }
}

fn exists(index: usize, registry: &ImageRegistry) -> Vec<RenderedStep> {
    let id = format!("exists-{index}");
    let missing = expression(&format!("steps.{id}.outcome == 'failure'"));
    let env = vec![
        ("IMAGE".to_owned(), Value::Text(registry.reference())),
        (
            "COMMIT".to_owned(),
            Value::Expr("${{ github.sha }}".to_owned()),
        ),
    ];
    vec![
        RenderedStep {
            id: Some(id),
            env: env.clone(),
            continue_on_error: true,
            ..command(
                format!("Look for the commit image in {}", registry.host.name()),
                "docker buildx imagetools inspect \"$IMAGE:sha-$COMMIT\"".to_owned(),
            )
        },
        RenderedStep {
            condition: Some(missing.clone()),
            env,
            ..command(
                format!("Explain the missing commit image in {}", registry.host.name()),
                "echo \"::error title=Image::$IMAGE:sha-$COMMIT does not exist because a superseded commit gets no image, so re-run the push workflow run of commit $COMMIT, then re-run this job\"".to_owned(),
            )
        },
        RenderedStep {
            condition: Some(missing),
            ..command(
                format!("Stop: no commit image in {}", registry.host.name()),
                "exit 1".to_owned(),
            )
        },
    ]
}
