// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::image_workflow::{edited, image, jobs, rendered, step_named, steps, uses_of};

const SHARED: &str = "image-multi-bin-svc-build";
const OWN: &str = "image-migrate-job-build";

fn builds(workflow: &serde_json::Value) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(jobs(workflow)?
        .keys()
        .filter(|id| id.ends_with("-build"))
        .cloned()
        .collect())
}

fn compiles(job: &serde_json::Value) -> usize {
    steps(job)
        .iter()
        .filter(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("cargo zigbuild"))
        })
        .count()
}

#[test]
fn images_with_one_compilation_identity_compile_once_and_stage_one_directory_each() -> TestResult {
    let workflow = rendered("image-workspace")?;
    assert_eq!(builds(&workflow)?, [SHARED]);
    let build = &workflow["jobs"][SHARED];
    assert_eq!(compiles(build), 1);
    let compile = step_named(build, "Compile")?["run"].as_str().unwrap_or("");
    assert_eq!(compile.matches("--bin legacy-migration").count(), 1);
    assert_eq!(compile.matches("--bin legacy-server").count(), 1);
    let stages: Vec<&str> = steps(build)
        .iter()
        .filter_map(|step| step["run"].as_str())
        .filter(|run| run.contains("image stage"))
        .collect();
    assert_eq!(stages.len(), 2);
    assert!(
        stages
            .iter()
            .any(|run| run.ends_with("--to dist --bin legacy-server --bin legacy-migration"))
    );
    assert!(
        stages
            .iter()
            .any(|run| run.ends_with("--to dist-migrate --bin legacy-migration"))
    );
    let artifacts: Vec<&str> = uses_of(build, "actions/upload-artifact")
        .iter()
        .filter_map(|step| step["with"]["name"].as_str())
        .collect();
    assert_eq!(artifacts, ["image-multi-bin-svc", "image-migrate-job"]);
    for part in ["publish", "check"] {
        let job = &workflow["jobs"][format!("image-migrate-job-{part}")];
        let needs = job["needs"].to_string();
        assert!(needs.contains(SHARED), "{part}: {needs}");
    }
    let download = &uses_of(
        &workflow["jobs"]["image-migrate-job-publish"],
        "actions/download-artifact",
    )[0]["with"];
    assert_eq!(download["name"], "image-migrate-job");
    assert_eq!(download["path"], "dist-migrate");
    Ok(())
}

#[test]
fn images_that_differ_in_the_compilation_or_share_a_stage_directory_compile_separately()
-> TestResult {
    for (change, from, to) in [
        (
            "features",
            "features = [\"tls\"]\ndockerfile = \"docker/Dockerfile.migrate\"",
            "dockerfile = \"docker/Dockerfile.migrate\"",
        ),
        (
            "glibc",
            "dockerfile = \"docker/Dockerfile.migrate\"\nstage = \"dist-migrate\"\nglibc = \"2.36\"",
            "dockerfile = \"docker/Dockerfile.migrate\"\nstage = \"dist-migrate\"\nglibc = \"2.38\"",
        ),
        (
            "stage",
            "stage = \"dist-migrate\"",
            "stage = \"dist/migrate\"",
        ),
    ] {
        let view = edited("image-workspace", |text| text.replace(from, to))?;
        let workflow = image(&view)?;
        let mut found = builds(&workflow)?;
        found.sort();
        assert_eq!(found, [OWN, SHARED], "{change}");
        for id in [OWN, SHARED] {
            assert_eq!(compiles(&workflow["jobs"][id]), 1, "{change} {id}");
        }
        assert!(
            workflow["jobs"]["image-migrate-job-publish"]["needs"]
                .to_string()
                .contains(OWN),
            "{change}"
        );
    }
    Ok(())
}
