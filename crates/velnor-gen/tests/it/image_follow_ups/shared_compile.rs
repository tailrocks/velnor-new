// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::{TargetKind, TargetName};

use crate::facts::TestResult;
use crate::image_workflow::{edited, image, jobs, rendered, step_named, steps, uses_of};

const SHARED: &str = "image-multi-bin-svc-build";
const OWN: &str = "image-ops-job-build";
const OPS: &str = "dockerfile = \"docker/Dockerfile.ops\"\nstage = \"dist-ops\"\nglibc = \"2.36\"\nopenssl = \"sysroot\"\npr_build = \"inputs\"";

fn builds(workflow: &Value) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut found: Vec<String> = jobs(workflow)?
        .keys()
        .filter(|id| id.ends_with("-build"))
        .cloned()
        .collect();
    found.sort();
    Ok(found)
}

fn compiles(job: &Value) -> usize {
    steps(job)
        .iter()
        .filter(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("cargo zigbuild"))
        })
        .count()
}

fn separate(change: &str, from: &str, to: &str) -> TestResult {
    let view = edited("image-workspace", |text| text.replace(from, to))?;
    let workflow = image(&view)?;
    assert_eq!(builds(&workflow)?, [SHARED, OWN], "{change}");
    for id in [OWN, SHARED] {
        assert_eq!(compiles(&workflow["jobs"][id]), 1, "{change} {id}");
    }
    let needs = workflow["jobs"]["image-ops-job-publish"]["needs"].to_string();
    assert!(needs.contains(OWN), "{change}");
    Ok(())
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
    assert!(stages.iter().all(|run| {
        run.ends_with("--private-under \"$RUNNER_TEMP\" --bin legacy-server --bin legacy-migration")
    }));
    let ids: Vec<&str> = steps(build)
        .iter()
        .filter_map(|step| step["id"].as_str())
        .filter(|id| id.starts_with("stage-"))
        .collect();
    assert_eq!(ids, ["stage-multi-bin-svc", "stage-ops-job"]);
    let artifacts: Vec<&str> = uses_of(build, "actions/upload-artifact")
        .iter()
        .filter_map(|step| step["with"]["name"].as_str())
        .collect();
    assert_eq!(artifacts, ["image-multi-bin-svc", "image-ops-job"]);
    for part in ["publish", "check"] {
        let needs = workflow["jobs"][format!("image-ops-job-{part}")]["needs"].to_string();
        assert!(needs.contains(SHARED), "{part}: {needs}");
    }
    let publish = &workflow["jobs"]["image-ops-job-publish"];
    let download = &uses_of(publish, "actions/download-artifact")[0]["with"];
    assert_eq!(download["name"], "image-ops-job");
    assert_eq!(download["path"], "dist-ops");
    Ok(())
}

#[test]
fn images_that_differ_in_the_compilation_or_share_a_stage_directory_compile_separately()
-> TestResult {
    separate("features", &format!("features = [\"tls\"]\n{OPS}"), OPS)?;
    separate("glibc", OPS, &OPS.replace("2.36", "2.38"))?;
    separate("stage", "stage = \"dist-ops\"", "stage = \"dist/ops\"")?;
    Ok(())
}

#[test]
fn images_that_build_different_package_sets_or_pull_request_policies_compile_separately()
-> TestResult {
    separate(
        "packages",
        "bins = [\"legacy-server\", \"legacy-migration\"]\nroot = \"backend-rust\"\nfeatures = [\"tls\"]\ndockerfile = \"docker/Dockerfile.ops\"",
        "bins = [\"legacy-migration\"]\nroot = \"backend-rust\"\nfeatures = [\"tls\"]\ndockerfile = \"docker/Dockerfile.ops\"",
    )?;
    separate(
        "pr_build",
        OPS,
        &OPS.replace("pr_build = \"inputs\"", "pr_build = \"off\""),
    )?;
    let workflow = image(&edited("image-workspace", |text| {
        text.replace(
            OPS,
            &OPS.replace("pr_build = \"inputs\"", "pr_build = \"off\""),
        )
    })?)?;
    assert!(workflow["jobs"].get("image-ops-job-check").is_none());
    assert!(
        !workflow["jobs"][OWN]["if"]
            .as_str()
            .unwrap_or_default()
            .contains("pull_request")
    );
    Ok(())
}

#[test]
fn a_bin_name_that_two_packages_both_own_is_never_merged_into_one_compile() -> TestResult {
    let mut view = edited("image-workspace", |text| {
        text.replacen(
            "bins = [\"legacy-server\", \"legacy-migration\"]",
            "bins = [\"legacy-migration\"]\npackage = \"legacy-migrate\"",
            1,
        )
        .replacen(
            "bins = [\"legacy-server\", \"legacy-migration\"]",
            "bins = [\"legacy-migration\"]\npackage = \"legacy-server\"",
            1,
        )
    })?;
    for metadata in view.rust_metadata.values_mut() {
        for member in &mut metadata.members {
            if member.name.as_str() == "legacy-server" {
                let mut twin = member
                    .targets
                    .iter()
                    .find(|target| target.kinds.contains(&TargetKind::Bin))
                    .cloned()
                    .ok_or("no bin")?;
                twin.name = TargetName::new("legacy-migration")?;
                member.targets.push(twin);
            }
        }
    }
    let workflow = image(&view)?;
    assert_eq!(builds(&workflow)?, [SHARED, OWN]);
    let own = step_named(&workflow["jobs"][OWN], "Compile")?["run"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let shared = step_named(&workflow["jobs"][SHARED], "Compile")?["run"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    assert!(
        own.contains("--package legacy-server") && !own.contains("--package legacy-migrate"),
        "{own}"
    );
    assert!(
        shared.contains("--package legacy-migrate") && !shared.contains("--package legacy-server"),
        "{shared}"
    );
    Ok(())
}
