// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

const TARGET_VARIABLE: &str = "CARGO_TARGET_DIR";

fn jobs(text: &str) -> Result<Vec<(String, Value)>, Box<dyn std::error::Error>> {
    let yaml: Value = serde_saphyr::from_str(text)?;
    Ok(yaml["jobs"]
        .as_object()
        .ok_or("jobs")?
        .iter()
        .map(|(id, job)| (id.clone(), job.clone()))
        .collect())
}

fn steps(job: &Value) -> &[Value] {
    job["steps"].as_array().map_or(&[], Vec::as_slice)
}

fn step_targets(job: &Value) -> Vec<&str> {
    steps(job)
        .iter()
        .filter_map(|step| step["env"][TARGET_VARIABLE].as_str())
        .collect()
}

fn effective<'a>(job: &'a Value, step: &'a Value) -> Option<&'a str> {
    step["env"][TARGET_VARIABLE]
        .as_str()
        .or_else(|| job["env"][TARGET_VARIABLE].as_str())
}

#[test]
fn a_job_whose_cargo_steps_share_one_root_sets_the_target_directory_once_at_job_level() -> TestResult
{
    let mut single = 0;
    for (id, job) in jobs(&workflow(&view(&["Cargo.toml"], None)?)?)? {
        let job_level = job["env"][TARGET_VARIABLE].as_str();
        if job_level.is_some() {
            single += 1;
            assert_eq!(
                job["env"].as_object().map(serde_json::Map::len),
                Some(1),
                "{id}"
            );
            assert!(
                step_targets(&job).is_empty(),
                "{id}: {:?}",
                step_targets(&job)
            );
        }
    }
    assert!(
        single >= 1,
        "at least one single-root job renders job-level env"
    );
    Ok(())
}

#[test]
fn a_job_that_spans_roots_keeps_each_cargo_step_target_directory() -> TestResult {
    let mut mixed = 0;
    for (id, job) in jobs(&workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?)? {
        let targets = step_targets(&job);
        let distinct: std::collections::BTreeSet<_> = targets.iter().collect();
        if distinct.len() > 1 {
            mixed += 1;
            assert!(job["env"][TARGET_VARIABLE].is_null(), "{id}");
        }
        if job["env"][TARGET_VARIABLE].is_string() {
            assert!(targets.is_empty(), "{id}");
        }
    }
    assert!(
        mixed >= 1,
        "a job spanning two roots keeps step-level values"
    );
    Ok(())
}

#[test]
fn job_level_target_directory_changes_no_cargo_step_effective_value() -> TestResult {
    for manifests in [&["Cargo.toml"][..], &["Cargo.toml", "lab/Cargo.toml"][..]] {
        for (id, job) in jobs(&workflow(&view(manifests, None)?)?)? {
            for step in steps(&job).iter().filter(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains(" -- cargo ") && !run.contains("cargo fetch"))
            }) {
                let directory = step["working-directory"].as_str();
                let expected = format!(
                    "${{{{ github.workspace }}}}/{}",
                    directory.map_or_else(|| "target".to_owned(), |dir| format!("{dir}/target"))
                );
                let actual = effective(&job, step);
                if actual.is_some() {
                    assert_eq!(actual, Some(expected.as_str()), "{id}: {}", step["name"]);
                }
            }
        }
    }
    Ok(())
}
