// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;
use velnor_model::{RepoPath, RepoView};

use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const WORKSPACE: &str = "${{ github.workspace }}/";
const TARGET_VARIABLE: &str = "CARGO_TARGET_DIR";
const NON_BUILDING: [&str; 3] = ["cargo-fmt", "cargo-deny", "cargo-machete"];

fn with_target_dir(
    repo: &mut RepoView,
    config: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let config: RepoPath = path(config)?;
    repo.tracked_paths.insert(config.clone());
    repo.files.insert(config, text.as_bytes().to_vec());
    Ok(())
}

type JobSteps = Vec<(String, Vec<Value>)>;

fn steps_of(text: &str) -> Result<JobSteps, Box<dyn std::error::Error>> {
    let yaml: Value = serde_saphyr::from_str(text)?;
    let jobs = yaml["jobs"].as_object().ok_or("jobs")?;
    Ok(jobs
        .iter()
        .map(|(id, job)| {
            (
                id.clone(),
                job["steps"].as_array().cloned().unwrap_or_default(),
            )
        })
        .collect())
}

fn builds(step: &Value) -> bool {
    let Some(run) = step["run"].as_str() else {
        return false;
    };
    run.split(" -- ")
        .skip(1)
        .find(|segment| segment.starts_with("cargo"))
        .is_some_and(|command| {
            !NON_BUILDING
                .iter()
                .any(|tool| command.split_whitespace().next() == Some(*tool))
        })
}

fn target_of(step: &Value) -> Option<&str> {
    step["env"][TARGET_VARIABLE].as_str()
}

fn cache_paths(steps: &[Value]) -> BTreeSet<String> {
    steps
        .iter()
        .filter(|step| {
            step["name"]
                .as_str()
                .is_some_and(|name| name.ends_with("the target cache"))
        })
        .filter_map(|step| step["with"]["path"].as_str().map(str::to_owned))
        .collect()
}

fn two_roots() -> Result<String, Box<dyn std::error::Error>> {
    workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)
}

fn two_roots_custom_lab_target() -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    with_target_dir(
        &mut repo,
        "lab/.cargo/config.toml",
        "[build]\ntarget-dir = \"build/lab-out\"\n",
    )?;
    workflow(&repo)
}

fn expected_target(directory: Option<&str>, custom: bool) -> String {
    let relative = match (directory, custom) {
        (None, _) => "target".to_owned(),
        (Some(directory), false) => format!("{directory}/target"),
        (Some(directory), true) => format!("{directory}/build/lab-out"),
    };
    format!("{WORKSPACE}{relative}")
}

#[test]
fn every_cargo_step_of_a_root_builds_into_that_roots_target_directory() -> TestResult {
    for custom in [false, true] {
        let text = if custom {
            two_roots_custom_lab_target()?
        } else {
            two_roots()?
        };
        let mut seen = BTreeSet::new();
        for (job, steps) in steps_of(&text)? {
            for step in steps.iter().filter(|step| builds(step)) {
                let directory = step["working-directory"].as_str();
                assert_eq!(
                    target_of(step),
                    Some(expected_target(directory, custom).as_str()),
                    "{job}: {}",
                    step["name"]
                );
                seen.insert(directory.map(str::to_owned));
            }
        }
        assert_eq!(
            seen,
            BTreeSet::from([None, Some("lab".to_owned())]),
            "both roots run cargo, custom target {custom}"
        );
    }
    Ok(())
}

#[test]
fn steps_that_do_not_build_get_no_target_directory() -> TestResult {
    for (job, steps) in steps_of(&two_roots()?)? {
        for step in steps.iter().filter(|step| !builds(step)) {
            assert_eq!(target_of(step), None, "{job}: {}", step["name"]);
        }
    }
    Ok(())
}

#[test]
fn the_cache_steps_and_the_cargo_steps_of_a_job_agree_on_each_roots_target_path() -> TestResult {
    for text in [two_roots()?, two_roots_custom_lab_target()?] {
        let mut checked = 0;
        for (job, steps) in steps_of(&text)? {
            let cargo: BTreeSet<String> = steps
                .iter()
                .filter(|step| builds(step))
                .filter_map(|step| target_of(step))
                .map(|value| value.trim_start_matches(WORKSPACE).to_owned())
                .collect();
            if cargo.is_empty() {
                continue;
            }
            checked += 1;
            assert_eq!(cargo, cache_paths(&steps), "{job}");
        }
        assert!(
            checked >= 2,
            "the root jobs and the audit job were compared"
        );
    }
    Ok(())
}
