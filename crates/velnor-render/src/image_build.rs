// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use velnor_model::{ImageConfig, ImageOpenssl, ImagePrBuild};

use crate::cargo::offline_env;
use crate::helper_steps::{mbx_private_step, mbx_seed_step, mbx_store_step, mirror_step};
use crate::image::{
    Events, IMAGE_BUILDKIT, IMAGE_SYSROOT_LOCK, IMAGE_TARGET, ImageBuild, ImageMbx, ImageWorkflow,
};
use crate::image_group::Compile;
use crate::image_publish::{TIP_STEP, tip};
use crate::install_steps::{install_helper, install_tools};
use crate::mbx_out_dirs::mbx_out_dirs_step;
use crate::pinned_actions::DOCKER_SETUP_BUILDX;
use crate::private_steps::{release_mint, release_phase};
use crate::release_binaries::{command, upload};
use crate::release_steps::{expression, join, permissions, shell_word};
use crate::rules::{RenderedConcurrency, RenderedJob, RenderedStep, Value};
use crate::steps::{CHECKOUT, RUN};

const ARTIFACT_RETENTION_DAYS: u64 = 7;
pub(crate) const LIVE_OUTPUT: &str = "live";

pub(crate) fn job_id(image: &ImageConfig, part: &str) -> String {
    format!("image-{}-{part}", image.name)
}

pub(crate) fn artifact(image: &ImageConfig) -> String {
    format!("image-{}", image.name)
}

pub(crate) fn staged(image: &ImageConfig) -> String {
    if image.context == "." {
        image.stage.clone()
    } else {
        join(&image.context, &image.stage)
    }
}

pub(crate) fn labels() -> Value {
    Value::Expr(
        "org.opencontainers.image.source=${{ github.server_url }}/${{ github.repository }}\norg.opencontainers.image.revision=${{ github.sha }}"
            .to_owned(),
    )
}

pub(crate) fn setup_buildx() -> RenderedStep {
    RenderedStep {
        with: vec![(
            "driver-opts".to_owned(),
            Value::Text(format!("image={IMAGE_BUILDKIT}")),
        )],
        ..DOCKER_SETUP_BUILDX.step("Set up Docker Buildx".to_owned())
    }
}

pub(crate) fn checkout() -> RenderedStep {
    RenderedStep {
        name: "Check out".to_owned(),
        uses: Some(CHECKOUT.to_owned()),
        with: vec![
            ("fetch-depth".to_owned(), Value::Int(1)),
            ("persist-credentials".to_owned(), Value::Bool(false)),
        ],
        ..RenderedStep::default()
    }
}

pub(crate) fn sha_scope() -> &'static str {
    "${{ github.event_name == 'push' && github.sha || github.run_id }}"
}

pub(crate) fn stands_down() -> RenderedStep {
    tip(Some(expression(
        "github.event_name == 'push' && github.run_attempt == 1",
    )))
}

pub(crate) fn only_live(mut step: RenderedStep) -> RenderedStep {
    let live = format!(
        "github.event_name != 'push' || github.run_attempt != 1 || steps.{TIP_STEP}.outputs.ready == 'true'"
    );
    let inner = step.condition.as_deref().map(|text| {
        text.strip_prefix("${{ ")
            .and_then(|rest| rest.strip_suffix(" }}"))
            .unwrap_or(text)
            .to_owned()
    });
    step.condition = Some(expression(&match inner {
        Some(inner) => format!("({inner}) && ({live})"),
        None => live,
    }));
    step
}

pub(crate) fn build_job(
    workflow: &ImageWorkflow<'_>,
    group: &Compile<'_>,
    events: &Events<'_>,
) -> RenderedJob {
    let reached = if group
        .members
        .iter()
        .all(|build| build.image.pr_build == ImagePrBuild::Off)
    {
        events.branch_push()
    } else {
        format!("({}) || ({})", events.branch_push(), events.pull_request())
    };
    let id = group.id();
    RenderedJob {
        concurrency: Some(RenderedConcurrency {
            group: format!("{id}-{}", sha_scope()),
            cancel_in_progress: true,
        }),
        outputs: vec![(
            LIVE_OUTPUT.to_owned(),
            format!(
                "${{{{ github.run_attempt != 1 || steps.{TIP_STEP}.outputs.ready == 'true' }}}}"
            ),
        )],
        id,
        condition: Some(expression(&reached)),
        permissions: permissions(&[("contents", "read")]),
        runs_on: workflow.runner.to_string(),
        timeout_minutes: group
            .members
            .iter()
            .map(|build| build.image.timeout_minutes)
            .max()
            .unwrap_or_default(),
        steps: build_steps(workflow, group),
        ..RenderedJob::default()
    }
}

fn build_steps(workflow: &ImageWorkflow<'_>, group: &Compile<'_>) -> Vec<RenderedStep> {
    let first = group.members[0];
    let mut steps = vec![checkout(), install_helper(workflow.helper), stands_down()];
    match workflow.mbx {
        ImageMbx::Off => {}
        ImageMbx::Private => steps.extend([mbx_private_step(), mbx_out_dirs_step()]),
        ImageMbx::Shared => steps.extend([mbx_store_step(), mbx_seed_step()]),
    }
    steps.extend(workflow.mirror.map(mirror_step));
    if let Some(sources) = workflow.private {
        steps.extend(release_phase(
            sources,
            &join(first.root, "Cargo.toml"),
            release_mint(workflow.helper),
        ));
    }
    steps.push(install_tools(None));
    steps.extend(workflow.mirror.map(mirror_step));
    steps.extend([command(
        format!("Add the {IMAGE_TARGET} target"),
        format!("rustup target add {IMAGE_TARGET}"),
    )]);
    if first.image.openssl == ImageOpenssl::Sysroot {
        steps.push(command(
            "Unpack the pinned OpenSSL sysroot".to_owned(),
            format!(
                "velnor-ci image sysroot --lock {IMAGE_SYSROOT_LOCK} --out \"$RUNNER_TEMP/image-sysroot\""
            ),
        ));
    }
    steps.push(compile(workflow, group));
    for build in &group.members {
        steps.push(stage(build));
        let mut artifact_step = upload(artifact(build.image), staged(build.image));
        for (name, value) in &mut artifact_step.with {
            if name == "retention-days" {
                *value = Value::Int(ARTIFACT_RETENTION_DAYS);
            }
        }
        steps.push(artifact_step);
    }
    let live = steps.split_off(3);
    steps.extend(live.into_iter().map(only_live));
    steps
}

fn compile(workflow: &ImageWorkflow<'_>, group: &Compile<'_>) -> RenderedStep {
    let build = group.members[0];
    let image = build.image;
    let offline = workflow.private.is_some();
    let profile = if image.profile == "release" {
        "--release".to_owned()
    } else {
        format!("--profile {}", image.profile)
    };
    let mut run = format!(
        "{RUN} cargo zigbuild {profile} --locked{} --target-dir target --target {IMAGE_TARGET}.{}",
        if offline { " --offline" } else { "" },
        image.glibc
    );
    for package in group.packages() {
        let _ = write!(run, " --package {}", shell_word(package));
    }
    for bin in group.bins() {
        let _ = write!(run, " --bin {}", shell_word(bin));
    }
    if !image.default_features {
        run.push_str(" --no-default-features");
    }
    if !image.features.is_empty() {
        let _ = write!(run, " --features {}", shell_word(&image.features.join(",")));
    }
    let mut env = vec![
        ("CARGO_INCREMENTAL".to_owned(), Value::Text("0".to_owned())),
        (
            "CARGO_TERM_COLOR".to_owned(),
            Value::Text("never".to_owned()),
        ),
        ("SOURCE_DATE_EPOCH".to_owned(), Value::Text("0".to_owned())),
    ];
    env.extend(offline.then(offline_env));
    let name = group
        .members
        .iter()
        .map(|member| member.image.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    RenderedStep {
        working_directory: (build.root != ".").then(|| build.root.to_owned()),
        env,
        ..command(format!("Compile {name} with zigbuild"), run)
    }
}

fn stage(build: &ImageBuild<'_>) -> RenderedStep {
    let image = build.image;
    let mut run = format!(
        "velnor-ci image stage --target-dir {} --profile {} --glibc {} --openssl {} --to {}",
        shell_word(&join(build.root, "target")),
        shell_word(image.profile_directory()),
        image.glibc,
        image.openssl.name(),
        shell_word(&staged(image))
    );
    for bin in &image.bins {
        let _ = write!(run, " --bin {}", shell_word(bin));
    }
    command(
        format!(
            "Stage the {} binaries and check their glibc ceiling",
            image.name
        ),
        run,
    )
}
