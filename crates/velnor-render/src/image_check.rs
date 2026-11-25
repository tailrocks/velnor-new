// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::ImagePrBuild;

use crate::image::{Events, ImageBuild, ImageWorkflow};
use crate::image_build::{artifact, job_id, labels, setup_buildx, staged};
use crate::image_changes::{CHANGES_JOB, fail_when, failed, selected, selective};
use crate::image_publish::{host_guard, restore_modes, sparse_checkout};
use crate::image_stage::verify_step;
use crate::install_steps::install_helper;
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
    let picked = selected(image).filter(|_| selective(workflow));
    let failure = match &picked {
        Some(_) => format!("{} || {}", failed(compile), failed(CHANGES_JOB)),
        None => failed(compile),
    };
    let mut steps = vec![fail_when(
        &failure,
        "Fail because a job that this check needs failed",
    )];
    steps.extend(host_guard());
    steps.extend([
        sparse_checkout(image),
        install_helper(workflow.helper),
        download_to(artifact(image), &staged(image)),
        verify_step(compile, image),
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
    ]);
    Some(RenderedJob {
        id: job_id(image, "check"),
        needs: [compile.to_owned()]
            .into_iter()
            .chain(picked.is_some().then(|| CHANGES_JOB.to_owned()))
            .collect(),
        condition: Some(expression(&match picked {
            Some(picked) => format!(
                "!cancelled() && ({}) && ({picked} || {} || {})",
                events.checked(),
                failed(CHANGES_JOB),
                failed(compile)
            ),
            None => format!(
                "!cancelled() && ({}) && needs.{compile}.result != 'skipped'",
                events.checked()
            ),
        })),
        permissions: permissions(&[("contents", "read")]),
        runs_on: workflow.runner.to_string(),
        timeout_minutes: CHECK_TIMEOUT_MINUTES,
        steps,
        ..RenderedJob::default()
    })
}
