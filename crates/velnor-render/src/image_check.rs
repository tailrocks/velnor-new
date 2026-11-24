// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::ImagePrBuild;

use crate::image::{Events, ImageBuild, ImageWorkflow};
use crate::image_build::{artifact, job_id, labels, setup_buildx, staged};
use crate::image_publish::{restore_modes, sparse_checkout};
use crate::pinned_actions::DOCKER_BUILD_PUSH;
use crate::release_binaries::download_to;
use crate::release_steps::{expression, permissions};
use crate::rules::{RenderedJob, RenderedStep, Value};

const CHECK_TIMEOUT_MINUTES: u16 = 30;

pub(crate) fn check_job(
    workflow: &ImageWorkflow<'_>,
    build: &ImageBuild<'_>,
    compile: &str,
    events: &Events<'_>,
) -> Option<RenderedJob> {
    let image = build.image;
    if image.pr_build == ImagePrBuild::Off {
        return None;
    }
    let steps = vec![
        sparse_checkout(image),
        download_to(artifact(image), &staged(image)),
        restore_modes(image),
        setup_buildx(),
        RenderedStep {
            with: vec![
                ("context".to_owned(), Value::Text(image.context.clone())),
                ("file".to_owned(), Value::Text(image.dockerfile.clone())),
                (
                    "platforms".to_owned(),
                    Value::Text("linux/amd64".to_owned()),
                ),
                ("push".to_owned(), Value::Bool(false)),
                ("provenance".to_owned(), Value::Bool(false)),
                ("sbom".to_owned(), Value::Bool(false)),
                ("labels".to_owned(), labels()),
            ],
            ..DOCKER_BUILD_PUSH.step(format!("Build the {} image without pushing", image.name))
        },
    ];
    Some(RenderedJob {
        id: job_id(image, "check"),
        needs: vec![compile.to_owned()],
        condition: Some(expression(&events.checked())),
        permissions: permissions(&[("contents", "read")]),
        runs_on: workflow.runner.to_string(),
        timeout_minutes: CHECK_TIMEOUT_MINUTES,
        steps,
        ..RenderedJob::default()
    })
}
