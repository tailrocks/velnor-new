// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{ImageConfig, ImageHost, ImageRegistry, ImageVisibility};

use crate::image::{Events, IMAGE_ENVIRONMENT, ImageBuild, ImageWorkflow};
use crate::image_build::{artifact, checkout, job_id, labels, setup_buildx, staged};
use crate::install_steps::install_helper;
use crate::pinned_actions::{DOCKER_BUILD_PUSH, DOCKER_LOGIN};
use crate::release_binaries::{command, download_to};
use crate::release_steps::{expression, permissions, variables};
use crate::rules::{RenderedJob, RenderedStep, Value};

const GATE_WAIT_SECONDS: u32 = 2400;
const GATE_TIMEOUT_MINUTES: u16 = 50;
const PUBLISH_TIMEOUT_MINUTES: u16 = 30;
const PROMOTE_TIMEOUT_MINUTES: u16 = 10;
const PREFLIGHT_STEP: &str = "preflight";
const PUSH_STEP: &str = "push";
const VERSION_STEP: &str = "version";
const GATE_STEP: &str = "gate";

fn gate_scopes() -> Vec<(String, String)> {
    permissions(&[
        ("actions", "read"),
        ("contents", "read"),
        ("statuses", "read"),
    ])
}

fn registry_scopes(image: &ImageConfig) -> Vec<(String, String)> {
    let mut scopes = vec![("contents", "read")];
    if image
        .registries
        .iter()
        .any(|registry| registry.host == ImageHost::Ghcr)
    {
        scopes.push(("packages", "write"));
    }
    permissions(&scopes)
}

fn gate_step(wait: u32, tip: bool) -> RenderedStep {
    let tip = if tip { " --require-tip" } else { "" };
    RenderedStep {
        id: Some(GATE_STEP.to_owned()),
        name: "Wait for every required check of the commit".to_owned(),
        env: variables(&[
            ("GITHUB_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("COMMIT", "${{ github.sha }}"),
        ]),
        run: Some(format!(
            "velnor-ci image gate --repo=\"$REPOSITORY\" --sha=\"$COMMIT\" --wait-seconds={wait}{tip}"
        )),
        ..RenderedStep::default()
    }
}

pub(crate) fn gate_job(
    workflow: &ImageWorkflow<'_>,
    build: &ImageBuild<'_>,
    events: &Events<'_>,
) -> RenderedJob {
    let image = build.image;
    RenderedJob {
        id: job_id(image, "gate"),
        condition: Some(expression(&events.branch_push())),
        permissions: gate_scopes(),
        outputs: vec![(
            "ready".to_owned(),
            format!("${{{{ steps.{GATE_STEP}.outputs.ready }}}}"),
        )],
        runs_on: workflow.runner.to_string(),
        timeout_minutes: GATE_TIMEOUT_MINUTES,
        steps: vec![
            install_helper(workflow.helper),
            gate_step(GATE_WAIT_SECONDS, true),
        ],
        ..RenderedJob::default()
    }
}

fn sparse_checkout(image: &ImageConfig) -> RenderedStep {
    let mut paths = vec![format!("/{}", image.dockerfile)];
    paths.push(if image.context == "." {
        "/.dockerignore".to_owned()
    } else {
        format!("/{}/.dockerignore", image.context)
    });
    paths.extend(image.inputs.iter().map(|input| format!("/{input}")));
    let mut step = checkout();
    "Check out the Dockerfile and its inputs".clone_into(&mut step.name);
    step.with.splice(
        0..0,
        [
            (
                "ref".to_owned(),
                Value::Expr("${{ github.sha }}".to_owned()),
            ),
            ("sparse-checkout".to_owned(), Value::Text(paths.join("\n"))),
            ("sparse-checkout-cone-mode".to_owned(), Value::Bool(false)),
        ],
    );
    step
}

fn private_hub(image: &ImageConfig) -> Option<&ImageRegistry> {
    image.registries.iter().find(|registry| {
        registry.host == ImageHost::DockerHub && registry.visibility == ImageVisibility::Private
    })
}

fn preflight(registry: &ImageRegistry) -> RenderedStep {
    let (Some(username), Some(token)) = (&registry.username, &registry.token) else {
        return RenderedStep::default();
    };
    RenderedStep {
        id: Some(PREFLIGHT_STEP.to_owned()),
        name: "Refuse to push to a Docker Hub repository that is missing or public".to_owned(),
        env: vec![
            (
                "VELNOR_HUB_USERNAME".to_owned(),
                Value::Expr(username.expression()),
            ),
            (
                "VELNOR_HUB_TOKEN".to_owned(),
                Value::Expr(token.expression()),
            ),
            (
                "IMAGE_TAG".to_owned(),
                Value::Expr("sha-${{ github.sha }}".to_owned()),
            ),
        ],
        run: Some(format!(
            "velnor-ci image preflight --repository {} --tag \"$IMAGE_TAG\"",
            registry.repository
        )),
        ..RenderedStep::default()
    }
}

fn login(registry: &ImageRegistry) -> RenderedStep {
    let name = format!("Log in to {}", registry.host.name());
    let step = DOCKER_LOGIN.step(name);
    let with = match (&registry.username, &registry.token) {
        (Some(username), Some(token)) => vec![
            ("username".to_owned(), Value::Expr(username.expression())),
            ("password".to_owned(), Value::Expr(token.expression())),
        ],
        _ => vec![
            ("registry".to_owned(), Value::Text("ghcr.io".to_owned())),
            (
                "username".to_owned(),
                Value::Expr("${{ github.actor }}".to_owned()),
            ),
            (
                "password".to_owned(),
                Value::Expr("${{ github.token }}".to_owned()),
            ),
        ],
    };
    RenderedStep { with, ..step }
}

fn tags(image: &ImageConfig, branch: &str) -> Value {
    let lines: Vec<String> = image
        .registries
        .iter()
        .flat_map(|registry| {
            let reference = registry.reference();
            [
                format!("{reference}:sha-${{{{ github.sha }}}}"),
                format!("{reference}:{branch}"),
            ]
        })
        .collect();
    Value::Expr(lines.join("\n"))
}

fn push(image: &ImageConfig, branch: &str) -> RenderedStep {
    let attested = image.attestations;
    let attestation = |on: &str| {
        if attested {
            Value::Text(on.to_owned())
        } else {
            Value::Bool(false)
        }
    };
    RenderedStep {
        id: Some(PUSH_STEP.to_owned()),
        with: vec![
            ("context".to_owned(), Value::Text(image.context.clone())),
            ("file".to_owned(), Value::Text(image.dockerfile.clone())),
            (
                "platforms".to_owned(),
                Value::Text("linux/amd64".to_owned()),
            ),
            ("push".to_owned(), Value::Bool(true)),
            ("provenance".to_owned(), attestation("mode=max")),
            ("sbom".to_owned(), Value::Bool(attested)),
            ("tags".to_owned(), tags(image, branch)),
            ("labels".to_owned(), labels()),
        ],
        ..DOCKER_BUILD_PUSH.step(format!("Build and push the {} image", image.name))
    }
}

fn report(registry: &ImageRegistry, fresh: Option<&str>) -> Vec<RenderedStep> {
    let reference = format!("{}:sha-${{{{ github.sha }}}}", registry.reference());
    let host = registry.host.name();
    vec![RenderedStep {
        condition: fresh.map(str::to_owned),
        env: vec![
            ("IMAGE".to_owned(), Value::Expr(reference)),
            (
                "DIGEST".to_owned(),
                Value::Expr(format!("${{{{ steps.{PUSH_STEP}.outputs.digest }}}}")),
            ),
        ],
        ..command(
            format!("Record the digest pushed to {host}"),
            "echo \"$IMAGE $DIGEST\" >> \"$GITHUB_STEP_SUMMARY\"".to_owned(),
        )
    }]
}

pub(crate) fn publish_job(
    workflow: &ImageWorkflow<'_>,
    build: &ImageBuild<'_>,
    events: &Events<'_>,
) -> RenderedJob {
    let image = build.image;
    let hub = private_hub(image);
    let fresh = hub.map(|_| {
        expression(&format!(
            "steps.{PREFLIGHT_STEP}.outputs.sha_exists != 'true'"
        ))
    });
    let on_fresh = |mut step: RenderedStep| {
        step.condition.clone_from(&fresh);
        step
    };
    let mut steps = vec![
        sparse_checkout(image),
        install_helper(workflow.helper),
        download_to(artifact(image), &staged(image)),
    ];
    steps.extend(hub.map(preflight));
    steps.push(on_fresh(setup_buildx()));
    steps.extend(
        image
            .registries
            .iter()
            .map(|registry| on_fresh(login(registry))),
    );
    steps.push(on_fresh(push(image, workflow.branch)));
    for registry in &image.registries {
        steps.extend(report(registry, fresh.as_deref()));
    }
    RenderedJob {
        id: job_id(image, "publish"),
        needs: vec![job_id(image, "build"), job_id(image, "gate")],
        condition: Some(expression(&format!(
            "needs.{}.outputs.ready == 'true' && {}",
            job_id(image, "gate"),
            events.branch_push()
        ))),
        environment: Some(IMAGE_ENVIRONMENT.to_owned()),
        permissions: registry_scopes(image),
        runs_on: workflow.runner.to_string(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..RenderedJob::default()
    }
}

pub(crate) fn authority_job(
    workflow: &ImageWorkflow<'_>,
    build: &ImageBuild<'_>,
    events: &Events<'_>,
) -> RenderedJob {
    let image = build.image;
    let strict = format!("^{}-v[0-9]+[.][0-9]+[.][0-9]+$", image.name);
    let strip = format!("version=${{REF_NAME#{}-v}}", image.name);
    let ref_name = || variables(&[("REF_NAME", "${{ github.ref_name }}")]);
    RenderedJob {
        id: job_id(image, "authority"),
        condition: Some(expression(&events.tag_push(image))),
        permissions: gate_scopes(),
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
            gate_step(0, false),
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
