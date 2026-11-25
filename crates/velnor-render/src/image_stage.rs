// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use velnor_model::ImageConfig;

use crate::image::{ImageBuild, ImageWorkflow};
use crate::image_build::{ARTIFACT_RETENTION_DAYS, artifact, staged};
use crate::image_changes::{selected, selective};
use crate::release_binaries::{command, upload};
use crate::release_steps::{expression, join, shell_word};
use crate::rules::{RenderedStep, Value};

pub(crate) fn member_steps(
    workflow: &ImageWorkflow<'_>,
    build: &ImageBuild<'_>,
) -> Vec<RenderedStep> {
    let mut out = Vec::new();
    let picked = selected(build.image)
        .filter(|_| selective(workflow))
        .map(|text| expression(&text));
    out.push(RenderedStep {
        condition: picked.clone(),
        ..stage(build)
    });
    out.push(RenderedStep {
        condition: picked.clone(),
        env: vec![
            (
                "STAGED".to_owned(),
                Value::Expr(format!(
                    "${{{{ steps.{}.outputs.dir }}}}",
                    stage_step(build.image)
                )),
            ),
            (
                "SUMS".to_owned(),
                Value::Expr(format!(
                    "${{{{ steps.{}.outputs.sums }}}}",
                    stage_step(build.image)
                )),
            ),
        ],
        ..command(
            format!(
                "Check that the {} directory is unchanged right before the upload",
                build.image.name
            ),
            "velnor-ci image verify --dir \"$STAGED\" --sums \"$SUMS\"".to_owned(),
        )
    });
    let mut artifact_step = upload(
        artifact(build.image),
        format!("${{{{ steps.{}.outputs.dir }}}}", stage_step(build.image)),
    );
    artifact_step.condition = picked;
    for (name, value) in &mut artifact_step.with {
        if name == "retention-days" {
            *value = Value::Int(ARTIFACT_RETENTION_DAYS);
        }
    }
    artifact_step
        .with
        .push(("include-hidden-files".to_owned(), Value::Bool(true)));
    out.push(artifact_step);
    out
}
fn stage(build: &ImageBuild<'_>) -> RenderedStep {
    let image = build.image;
    let mut run = format!(
        "velnor-ci image stage --target-dir {} --profile {} --glibc {} --openssl {} --private-under \"$RUNNER_TEMP\"",
        shell_word(&join(build.root, "target")),
        shell_word(image.profile_directory()),
        image.glibc,
        image.openssl.name(),
    );
    for bin in &image.bins {
        let _ = write!(run, " --bin {}", shell_word(bin));
    }
    RenderedStep {
        id: Some(stage_step(image)),
        ..command(
            format!(
                "Stage the {} binaries and check their glibc ceiling",
                image.name
            ),
            run,
        )
    }
}
pub(crate) fn stage_step(image: &ImageConfig) -> String {
    format!("stage-{}", image.name)
}
pub(crate) fn sums_output(image: &ImageConfig) -> String {
    format!("sums-{}", image.name)
}
pub(crate) fn verify_step(compile: &str, image: &ImageConfig) -> RenderedStep {
    RenderedStep {
        env: vec![(
            "SUMS".to_owned(),
            Value::Expr(format!(
                "${{{{ needs.{compile}.outputs.{} }}}}",
                sums_output(image)
            )),
        )],
        ..command(
            "Check that the downloaded binaries are the ones that were staged".to_owned(),
            format!(
                "velnor-ci image verify --dir {} --sums \"$SUMS\"",
                shell_word(&staged(image))
            ),
        )
    }
}
