// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_render::{RenderedJob, RenderedStep, Value, check_image_boundary};

use crate::facts::TestResult;
use crate::image_workflow::{rendered, step_named, steps};

#[test]
fn every_image_job_that_moves_an_artifact_or_logs_in_refuses_a_github_host_other_than_github_com()
-> TestResult {
    for (name, ids) in [
        (
            "image-ghcr",
            [
                "image-cluster-svc-build",
                "image-cluster-svc-check",
                "image-cluster-svc-publish",
                "image-cluster-svc-advance",
                "image-cluster-svc-promote",
            ],
        ),
        (
            "image-plain",
            [
                "image-plain-svc-build",
                "image-plain-svc-check",
                "image-plain-svc-publish",
                "image-plain-svc-advance",
                "image-plain-svc-promote",
            ],
        ),
    ] {
        let workflow = rendered(name)?;
        for id in ids {
            let job = &workflow["jobs"][id];
            let guard = step_named(job, "Refuse a GitHub host other than github.com")?;
            assert!(
                guard["if"].as_str().is_some_and(|text| {
                    text.contains("github.server_url != 'https://github.com'")
                }),
                "{name} {id}"
            );
            let names: Vec<&str> = steps(job)
                .iter()
                .filter_map(|step| step["name"].as_str())
                .collect();
            let refuse = names
                .iter()
                .position(|text| text.starts_with("Refuse a GitHub host"))
                .ok_or("no guard")?;
            let first_use = steps(job)
                .iter()
                .position(|step| {
                    step["uses"].as_str().is_some_and(|uses| {
                        uses.starts_with("actions/upload-artifact")
                            || uses.starts_with("actions/download-artifact")
                            || uses.starts_with("docker/")
                    })
                })
                .unwrap_or(usize::MAX);
            assert!(refuse < first_use, "{name} {id}");
        }
    }
    Ok(())
}

#[test]
fn private_source_binaries_are_never_staged_in_a_public_repository() -> TestResult {
    let private = rendered("image-ghcr")?;
    let build = &private["jobs"]["image-cluster-svc-build"];
    let guard = step_named(build, "Refuse to stage private-source binaries")?;
    assert!(
        guard["if"]
            .as_str()
            .is_some_and(|text| text.contains("github.event.repository.private != true")),
        "{guard}"
    );
    let names: Vec<&str> = steps(build)
        .iter()
        .filter_map(|step| step["name"].as_str())
        .collect();
    let refuse = names
        .iter()
        .position(|text| text.starts_with("Refuse to stage private-source"))
        .ok_or("no guard")?;
    let upload = names
        .iter()
        .position(|text| text.starts_with("Upload"))
        .ok_or("no upload")?;
    let fetch = names
        .iter()
        .position(|text| text.contains("private"))
        .unwrap_or(usize::MAX);
    assert!(refuse < upload && refuse <= fetch.max(refuse));
    let plain = rendered("image-plain")?;
    assert!(
        steps(&plain["jobs"]["image-plain-svc-build"])
            .iter()
            .all(|step| !step["name"]
                .as_str()
                .unwrap_or("")
                .starts_with("Refuse to stage private-source"))
    );
    Ok(())
}

fn step(run: &str) -> RenderedStep {
    RenderedStep {
        name: run.to_owned(),
        run: Some(run.to_owned()),
        ..RenderedStep::default()
    }
}

fn building() -> RenderedJob {
    RenderedJob {
        id: "image-x-build".to_owned(),
        permissions: vec![("contents".to_owned(), "read".to_owned())],
        steps: vec![
            RenderedStep {
                name: "token".to_owned(),
                env: vec![(
                    "GH_TOKEN".to_owned(),
                    Value::Expr("${{ steps.mint.outputs.token }}".to_owned()),
                )],
                run: Some("velnor-ci fetch".to_owned()),
                ..RenderedStep::default()
            },
            step("cargo zigbuild"),
            step("velnor-ci image stage"),
        ],
        ..RenderedJob::default()
    }
}

#[test]
fn the_generator_refuses_an_image_job_that_mixes_repository_code_with_credentials() {
    assert_eq!(check_image_boundary(&[building()]), Vec::new());
    let unsafe_jobs: Vec<(&str, RenderedJob)> = vec![
        (
            "an environment",
            RenderedJob {
                environment: Some("images".to_owned()),
                ..building()
            },
        ),
        (
            "a package write",
            RenderedJob {
                permissions: vec![
                    ("contents".to_owned(), "read".to_owned()),
                    ("packages".to_owned(), "write".to_owned()),
                ],
                ..building()
            },
        ),
        (
            "an id token",
            RenderedJob {
                permissions: vec![
                    ("contents".to_owned(), "read".to_owned()),
                    ("id-token".to_owned(), "write".to_owned()),
                ],
                ..building()
            },
        ),
        (
            "no stated permissions",
            RenderedJob {
                permissions: Vec::new(),
                ..building()
            },
        ),
        (
            "a job secret",
            RenderedJob {
                env: vec![(
                    "REGISTRY".to_owned(),
                    Value::Expr("${{ secrets.DOCKERHUB_TOKEN }}".to_owned()),
                )],
                ..building()
            },
        ),
    ];
    for (what, job) in unsafe_jobs {
        assert!(!check_image_boundary(&[job]).is_empty(), "{what}");
    }
    let mut late = building();
    late.steps.push(RenderedStep {
        name: "push".to_owned(),
        env: vec![(
            "DOCKERHUB_PASSWORD".to_owned(),
            Value::Expr("${{ secrets.DOCKERHUB_TOKEN }}".to_owned()),
        )],
        run: Some("velnor-ci image stage".to_owned()),
        ..RenderedStep::default()
    });
    assert!(
        !check_image_boundary(&[late]).is_empty(),
        "a credential after the code"
    );
    let login = RenderedJob {
        id: "image-x-publish".to_owned(),
        steps: vec![RenderedStep {
            uses: Some("docker/login-action@abc".to_owned()),
            ..RenderedStep::default()
        }],
        ..RenderedJob::default()
    };
    assert!(
        !check_image_boundary(&[login]).is_empty(),
        "a login outside an environment"
    );
}

#[test]
fn every_generated_image_workflow_keeps_credentials_away_from_the_jobs_that_run_repository_code()
-> TestResult {
    for name in crate::image_workflow::SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in crate::image_workflow::jobs(&workflow)? {
            let runs_code = steps(job).iter().any(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("cargo"))
            });
            if runs_code {
                assert!(job.get("environment").is_none(), "{name} {id}");
                let permissions = job["permissions"].as_object().ok_or("no permissions")?;
                assert!(
                    permissions
                        .keys()
                        .all(|key| key == "contents" || key == "pull-requests"),
                    "{name} {id}: {permissions:?}"
                );
            }
        }
    }
    Ok(())
}
