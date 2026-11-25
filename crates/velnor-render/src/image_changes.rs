// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use velnor_model::ImagePrBuild;

use crate::image::{Events, ImageWorkflow, image_paths};
use crate::install_steps::install_helper;
use crate::release::HOSTED_RUNNER;
use crate::release_binaries::command;
use crate::release_steps::{expression, permissions, variables};
use crate::rules::{RenderedJob, RenderedStep, Value};

pub(crate) const CHANGES_JOB: &str = "image-changes";
const CHANGES_STEP: &str = "changes";
const CHANGES_TIMEOUT_MINUTES: u16 = 10;

pub(crate) fn selective(workflow: &ImageWorkflow<'_>) -> bool {
    workflow
        .images
        .iter()
        .filter(|build| build.image.pr_build != ImagePrBuild::Off)
        .count()
        >= 2
}

pub(crate) fn changes_job(
    workflow: &ImageWorkflow<'_>,
    events: &Events<'_>,
) -> Option<RenderedJob> {
    if !selective(workflow) {
        return None;
    }
    let mut matches = String::new();
    let mut outputs = Vec::new();
    for build in &workflow.images {
        let image = build.image;
        if image.pr_build == ImagePrBuild::Off {
            continue;
        }
        let globs = if image.pr_build == ImagePrBuild::Always {
            vec!["**".to_owned()]
        } else {
            image_paths(build)
        };
        for glob in globs {
            let _ = writeln!(matches, "{}={glob}", image.name);
        }
        outputs.push((
            image.name.clone(),
            format!("${{{{ steps.{CHANGES_STEP}.outputs.{} }}}}", image.name),
        ));
    }
    Some(RenderedJob {
        id: CHANGES_JOB.to_owned(),
        condition: Some(expression(&format!(
            "({}) || ({})",
            events.branch_push(),
            events.checked()
        ))),
        permissions: permissions(&[("contents", "read"), ("pull-requests", "read")]),
        outputs,
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: CHANGES_TIMEOUT_MINUTES,
        steps: vec![
            install_helper(workflow.helper),
            RenderedStep {
                id: Some(CHANGES_STEP.to_owned()),
                env: variables(&[
                    ("GITHUB_TOKEN", "${{ github.token }}"),
                    ("REPOSITORY", "${{ github.repository }}"),
                    ("EVENT", "${{ github.event_name }}"),
                    ("PULL_REQUEST", "${{ github.event.pull_request.number }}"),
                ])
                .into_iter()
                .chain([("VELNOR_IMAGE_MATCHES".to_owned(), Value::Text(matches))])
                .collect(),
                ..command(
                    "Select the images that the pull request reaches".to_owned(),
                    "velnor-ci image changes --repo=\"$REPOSITORY\" --event=\"$EVENT\" --pull-request=\"$PULL_REQUEST\"".to_owned(),
                )
            },
        ],
        ..RenderedJob::default()
    })
}
