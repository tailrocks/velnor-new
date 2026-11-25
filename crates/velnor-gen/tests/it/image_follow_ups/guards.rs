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
    let mut login_then_code = building();
    login_then_code.steps.insert(
        0,
        RenderedStep {
            uses: Some("docker/login-action@abc".to_owned()),
            ..RenderedStep::default()
        },
    );
    assert!(
        !check_image_boundary(&[login_then_code]).is_empty(),
        "a login before the code in a job without an environment"
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

#[test]
fn the_downloaded_binaries_are_checked_against_the_hashes_that_the_build_job_staged() -> TestResult
{
    for (name, build, check, publish) in [
        (
            "image-plain",
            "image-plain-svc-build",
            "image-plain-svc-check",
            "image-plain-svc-publish",
        ),
        (
            "image-ghcr",
            "image-cluster-svc-build",
            "image-cluster-svc-check",
            "image-cluster-svc-publish",
        ),
    ] {
        let workflow = rendered(name)?;
        let image = build
            .trim_start_matches("image-")
            .trim_end_matches("-build");
        assert!(
            workflow["jobs"][build]["outputs"][format!("sums-{image}").as_str()]
                .as_str()
                .is_some_and(|text| text.contains(&format!("steps.stage-{image}.outputs.sums"))),
            "{name}"
        );
        let build_job = &workflow["jobs"][build];
        let order: Vec<&str> = steps(build_job)
            .iter()
            .filter_map(|step| step["name"].as_str())
            .collect();
        let staged = order
            .iter()
            .position(|text| text.starts_with("Stage the"))
            .ok_or("no stage")?;
        let rechecked = order
            .iter()
            .position(|text| text.contains("is unchanged right before the upload"))
            .ok_or("no recheck")?;
        let uploaded = order
            .iter()
            .position(|text| text.starts_with("Upload image-"))
            .ok_or("no upload")?;
        assert!(staged < rechecked && rechecked < uploaded, "{name}");
        for id in [check, publish] {
            let job = &workflow["jobs"][id];
            let names: Vec<&str> = steps(job)
                .iter()
                .filter_map(|step| step["name"].as_str())
                .collect();
            let download = names
                .iter()
                .position(|text| text.starts_with("Download image-"))
                .ok_or("no download")?;
            let verify = names
                .iter()
                .position(|text| text.starts_with("Check that the downloaded binaries"))
                .ok_or("no verify")?;
            let docker = steps(job)
                .iter()
                .position(|step| {
                    step["uses"]
                        .as_str()
                        .is_some_and(|uses| uses.starts_with("docker/"))
                })
                .ok_or("no docker step")?;
            assert!(download < verify && verify < docker, "{name} {id}");
            let step = step_named(job, "Check that the downloaded binaries")?;
            assert!(
                step["env"]["SUMS"].as_str().is_some_and(
                    |text| text.contains(&format!("needs.{build}.outputs.sums-{image}"))
                ),
                "{name} {id}"
            );
            assert!(
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("image verify --dir")),
                "{name} {id}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_ghcr_package_that_is_already_public_is_refused_before_the_push() -> TestResult {
    let both = rendered("image-ghcr")?;
    let publish = &both["jobs"]["image-cluster-svc-publish"];
    let names: Vec<&str> = steps(publish)
        .iter()
        .filter_map(|step| step["name"].as_str())
        .collect();
    let preflight = names
        .iter()
        .position(|text| text.starts_with("Refuse to push to a ghcr.io package that is public"))
        .ok_or("no ghcr preflight")?;
    let push = names
        .iter()
        .position(|text| text.starts_with("Build and push"))
        .ok_or("no push")?;
    assert!(preflight < push);
    let run = step_named(publish, "ghcr.io package that is public")?["run"]
        .as_str()
        .ok_or("no run")?;
    assert!(
        run.contains("image ghcr-preflight --repository ghcr.io/acme/cluster-svc"),
        "{run}"
    );
    let plain = rendered("image-plain")?;
    assert!(
        steps(&plain["jobs"]["image-plain-svc-publish"])
            .iter()
            .all(|step| !step["name"]
                .as_str()
                .unwrap_or("")
                .contains("ghcr.io package"))
    );
    Ok(())
}
