// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{ImageConfig, ImageCredential, ImageHost, ImageRegistry, ImageVisibility};

use crate::image::{Events, IMAGE_ENVIRONMENT, ImageBuild, ImageWorkflow};
use crate::image_build::{artifact, checkout, job_id, labels, setup_buildx, staged};
use crate::image_gate::GATE_JOB;
use crate::install_steps::install_helper;
use crate::pinned_actions::{DOCKER_BUILD_PUSH, DOCKER_LOGIN};
use crate::release_binaries::{command, download_to};
use crate::release_steps::{expression, permissions, shell_word, variables};
use crate::rules::{RenderedJob, RenderedStep, Value};

const PUBLISH_TIMEOUT_MINUTES: u16 = 30;
const PREFLIGHT_STEP: &str = "preflight";
const PUSH_STEP: &str = "push";
const TIP_STEP: &str = "tip";

pub(crate) fn registry_scopes(image: &ImageConfig) -> Vec<(String, String)> {
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

pub(crate) fn sparse_checkout(image: &ImageConfig) -> RenderedStep {
    let mut paths = vec![format!("/{}", image.dockerfile)];
    paths.push(format!("/{}.dockerignore", image.dockerfile));
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

pub(crate) fn restore_modes(image: &ImageConfig) -> RenderedStep {
    command(
        "Restore the executable bits of the staged binaries".to_owned(),
        format!("chmod 0755 {}", executables(image)),
    )
}

fn executables(image: &ImageConfig) -> String {
    image
        .bins
        .iter()
        .map(|bin| shell_word(&format!("{}/{bin}", staged(image))))
        .collect::<Vec<_>>()
        .join(" ")
}

struct Hub<'a> {
    registry: &'a ImageRegistry,
    username: &'a ImageCredential,
    token: &'a ImageCredential,
}

fn private_hub(image: &ImageConfig) -> Option<Hub<'_>> {
    image.registries.iter().find_map(|registry| {
        let private = registry.host == ImageHost::DockerHub
            && registry.visibility == ImageVisibility::Private;
        match (&registry.username, &registry.token) {
            (Some(username), Some(token)) if private => Some(Hub {
                registry,
                username,
                token,
            }),
            _ => None,
        }
    })
}

fn preflight(hub: &Hub<'_>) -> RenderedStep {
    let Hub {
        registry,
        username,
        token,
    } = hub;
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

pub(crate) fn login(registry: &ImageRegistry) -> RenderedStep {
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
                format!(
                    "${{{{ steps.{TIP_STEP}.outputs.ready == 'true' && '{reference}:{branch}' || '' }}}}"
                ),
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

fn tip() -> RenderedStep {
    RenderedStep {
        id: Some(TIP_STEP.to_owned()),
        env: variables(&[
            ("GITHUB_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
            ("COMMIT", "${{ github.sha }}"),
        ]),
        ..command(
            "Check that the commit is still the tip of the default branch".to_owned(),
            "velnor-ci image gate --repo=\"$REPOSITORY\" --sha=\"$COMMIT\" --only=tip".to_owned(),
        )
    }
}

fn withheld(branch: &str) -> Vec<RenderedStep> {
    let condition = expression(&format!("steps.{TIP_STEP}.outputs.ready != 'true'"));
    let notice = format!(
        "echo \"::notice title=Image::the {branch} tag was withheld because this commit is no longer the tip\""
    );
    let summary = format!(
        "echo \"The {branch} tag was withheld: this commit is no longer the tip of {branch}, so only sha-$COMMIT was pushed.\" >> \"$GITHUB_STEP_SUMMARY\""
    );
    vec![
        RenderedStep {
            condition: Some(condition.clone()),
            ..command(
                "Say that the default-branch tag was withheld".to_owned(),
                notice,
            )
        },
        RenderedStep {
            condition: Some(condition),
            env: variables(&[("COMMIT", "${{ github.sha }}")]),
            ..command(
                "Record that the default-branch tag was withheld".to_owned(),
                summary,
            )
        },
    ]
}

fn report(registry: &ImageRegistry) -> Vec<RenderedStep> {
    let reference = format!("{}:sha-${{{{ github.sha }}}}", registry.reference());
    let host = registry.host.name();
    vec![RenderedStep {
        env: vec![
            ("IMAGE".to_owned(), Value::Expr(reference)),
            (
                "DIGEST".to_owned(),
                Value::Expr(format!("${{{{ steps.{PUSH_STEP}.outputs.digest }}}}")),
            ),
            (
                "BRANCH_TAG".to_owned(),
                Value::Expr(format!(
                    "${{{{ steps.{TIP_STEP}.outputs.ready == 'true' && 'pushed' || 'withheld' }}}}"
                )),
            ),
        ],
        ..command(
            format!("Record the digest pushed to {host}"),
            "echo \"$IMAGE $DIGEST (default-branch tag $BRANCH_TAG)\" >> \"$GITHUB_STEP_SUMMARY\""
                .to_owned(),
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
    let mut steps = vec![
        sparse_checkout(image),
        install_helper(workflow.helper),
        download_to(artifact(image), &staged(image)),
    ];
    steps.extend(hub.as_ref().map(preflight));
    steps.push(restore_modes(image));
    steps.push(setup_buildx());
    steps.extend(image.registries.iter().map(login));
    steps.push(tip());
    steps.extend(withheld(workflow.branch));
    steps.push(push(image, workflow.branch));
    for registry in &image.registries {
        steps.extend(report(registry));
    }
    RenderedJob {
        id: job_id(image, "publish"),
        needs: vec![job_id(image, "build"), GATE_JOB.to_owned()],
        condition: Some(expression(&format!(
            "needs.{GATE_JOB}.outputs.ready == 'true' && {}",
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
