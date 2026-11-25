// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use velnor_model::ImageConfig;

use crate::image::{Events, IMAGE_ENVIRONMENT, ImageBuild, ImageWorkflow};
use crate::image_build::{checkout, job_id, setup_buildx};
use crate::image_gate::{GATE_JOB, GateOptions, gate_scopes, gate_step};
use crate::image_publish::{login, registry_scopes};
use crate::install_steps::install_helper;
use crate::release_binaries::command;
use crate::release_steps::{
    NO_PROGRAM_VARIABLES, TOOLCHAIN, expression, permissions, shell_word, variables,
};
use crate::rules::{RenderedConcurrency, RenderedJob, RenderedStep, Value};

const PROMOTE_TIMEOUT_MINUTES: u16 = 10;
const AUTHORITY_WAIT_SECONDS: u32 = 1200;
const AUTHORITY_WAIT_MINUTES: u16 = 30;
const VERSION_STEP: &str = "version";
const RELEASE_VERSION_HELPER: (u64, u64, u64) = (0, 1, 117);
const VERSION_CHECK_TIMEOUT_MINUTES: u16 = 10;

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
        timeout_minutes: AUTHORITY_WAIT_MINUTES,
        steps: vec![
            install_helper(workflow.helper),
            RenderedStep {
                env: ref_name(),
                ..command(
                    "Check that the tag names a strict version".to_owned(),
                    format!("[[ \"$REF_NAME\" =~ {strict} ]]"),
                )
            },
            authorize(image),
            gate_step(&GateOptions {
                wait: AUTHORITY_WAIT_SECONDS,
                tip: false,
                pr_only: &pr_only,
                verify: !pr_only.is_empty(),
                ignore: &[GATE_JOB.to_owned()],
                published: Some(&job_id(image, "publish")),
            }),
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

fn authorize(image: &ImageConfig) -> RenderedStep {
    let mut run = String::from(
        "velnor-ci image authorize --repo \"$REPOSITORY\" --tag \"$REF_NAME\" --actor \"$ACTOR\"",
    );
    for actor in &image.release_actors {
        let _ = write!(run, " --release-actor={}", shell_word(actor));
    }
    RenderedStep {
        env: variables(&[
            ("GITHUB_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("REF_NAME", "${{ github.ref_name }}"),
            ("ACTOR", "${{ github.actor }}"),
        ]),
        ..command(
            "Check that a trusted actor or a protected tag ruleset authorizes the release"
                .to_owned(),
            run,
        )
    }
}

fn checks_version(workflow: &ImageWorkflow<'_>) -> bool {
    workflow.helper.version().at_least(RELEASE_VERSION_HELPER)
}

pub(crate) fn version_job(
    workflow: &ImageWorkflow<'_>,
    build: &ImageBuild<'_>,
) -> Option<RenderedJob> {
    if !checks_version(workflow) {
        return None;
    }
    let image = build.image;
    let mut run = format!(
        "velnor-ci image release-version --version \"$VERSION\" --root {}",
        shell_word(build.root)
    );
    for bin in &image.bins {
        let _ = write!(run, " --bin={}", shell_word(bin));
    }
    if let Some(package) = &image.package {
        let _ = write!(run, " --package={}", shell_word(package));
    }
    let mut env = vec![
        (
            "VERSION".to_owned(),
            Value::Expr(format!(
                "${{{{ needs.{}.outputs.version }}}}",
                job_id(image, "authority")
            )),
        ),
        (
            "RUSTUP_TOOLCHAIN".to_owned(),
            Value::Text(TOOLCHAIN.to_owned()),
        ),
    ];
    env.extend(
        NO_PROGRAM_VARIABLES
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Value::Text((*value).to_owned()))),
    );
    let mut checkout = checkout();
    "Check out the tagged commit".clone_into(&mut checkout.name);
    checkout.with.splice(
        0..0,
        [(
            "ref".to_owned(),
            Value::Expr("${{ github.sha }}".to_owned()),
        )],
    );
    Some(RenderedJob {
        id: job_id(image, "version"),
        needs: vec![job_id(image, "authority")],
        permissions: permissions(&[("contents", "read")]),
        runs_on: workflow.control_runner(),
        timeout_minutes: VERSION_CHECK_TIMEOUT_MINUTES,
        steps: vec![
            checkout,
            install_helper(workflow.helper),
            command(
                "Install the Rust toolchain".to_owned(),
                format!("rustup toolchain install {TOOLCHAIN} --profile minimal --no-self-update"),
            ),
            RenderedStep {
                env,
                ..command(
                    "Refuse a release tag whose version is not the version of the crate that builds the image"
                        .to_owned(),
                    run,
                )
            },
        ],
        ..RenderedJob::default()
    })
}

pub(crate) fn promote_job(workflow: &ImageWorkflow<'_>, build: &ImageBuild<'_>) -> RenderedJob {
    let image = build.image;
    let mut steps = vec![install_helper(workflow.helper)];
    steps.extend(crate::image_publish::host_guard());
    steps.push(setup_buildx());
    steps.extend(image.registries.iter().map(login));
    let mut images = String::new();
    for registry in &image.registries {
        let _ = write!(images, " --image={}", shell_word(&registry.reference()));
    }
    steps.push(RenderedStep {
        env: vec![
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
            (
                "GITHUB_TOKEN".to_owned(),
                Value::Expr("${{ github.token }}".to_owned()),
            ),
            (
                "REPOSITORY".to_owned(),
                Value::Expr("${{ github.repository }}".to_owned()),
            ),
            (
                "REF_NAME".to_owned(),
                Value::Expr("${{ github.ref_name }}".to_owned()),
            ),
        ],
        ..command(
            "Promote the pushed digest unless the version already names another".to_owned(),
            format!("velnor-ci image promote --version \"$VERSION\" --commit \"$COMMIT\" --repo \"$REPOSITORY\" --tag \"$REF_NAME\"{images}"),
        )
    });
    RenderedJob {
        concurrency: Some(RenderedConcurrency {
            group: format!("{}-${{{{ github.ref }}}}", job_id(image, "promote")),
            cancel_in_progress: false,
        }),
        id: job_id(image, "promote"),
        needs: std::iter::once(job_id(image, "authority"))
            .chain(checks_version(workflow).then(|| job_id(image, "version")))
            .collect(),
        environment: Some(IMAGE_ENVIRONMENT.to_owned()),
        permissions: registry_scopes(image),
        runs_on: workflow.publish_runner(build),
        timeout_minutes: PROMOTE_TIMEOUT_MINUTES,
        steps,
        ..RenderedJob::default()
    }
}
