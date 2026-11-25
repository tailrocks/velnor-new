// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
