// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_gen::expected_with_notes;
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::golden::{load, samples};
use crate::yaml_tree::document;

const RELEASED: &str = "0.1.111";
const NEXT: &str = "0.1.112";
const STEP: &str = "velnor-ci check-configurations";
const WORKFLOW: &str = ".github/workflows/ci.yml";

struct Job {
    ids: Vec<String>,
    conditions: Vec<String>,
    checks: Vec<usize>,
}

fn render(
    pin: &str,
    testless: Option<&str>,
) -> Result<BTreeMap<String, Job>, Box<dyn std::error::Error>> {
    let mut view = load(&samples().join("check-configurations"))?;
    for metadata in view.rust_metadata.values_mut() {
        for member in &mut metadata.members {
            if testless.is_some_and(|root| member.manifest.as_str().starts_with(root)) {
                for target in &mut member.targets {
                    target.tests = false;
                }
            }
        }
    }
    let lock = velnor_rust::LOCK_PATH;
    let text = String::from_utf8(view.files.get(&lock).ok_or("no helper lock")?.clone())?;
    view.files
        .insert(lock, text.replace(RELEASED, pin).into_bytes());
    let (files, _) = expected_with_notes(&view)?;
    let text = files
        .get(&RepoPath::new(WORKFLOW)?)
        .ok_or("the workflow was not rendered")?;
    let jobs = document(text)?["jobs"].clone();
    let mut found = BTreeMap::new();
    for (id, job) in jobs.as_object().into_iter().flatten() {
        let steps = job["steps"].as_array().map_or(&[][..], Vec::as_slice);
        let checks: Vec<usize> = (0..steps.len())
            .filter(|index| steps[*index]["name"] == STEP)
            .collect();
        if !checks.is_empty() {
            let ids = steps
                .iter()
                .map(|step| step["id"].as_str().unwrap_or_default().to_owned())
                .collect();
            let conditions = steps
                .iter()
                .map(|step| step["if"].as_str().unwrap_or_default().to_owned())
                .collect();
            found.insert(
                id.clone(),
                Job {
                    ids,
                    conditions,
                    checks,
                },
            );
        }
    }
    Ok(found)
}

fn counts(found: &BTreeMap<String, Job>) -> Vec<(&str, usize)> {
    found
        .iter()
        .map(|(id, job)| (id.as_str(), job.checks.len()))
        .collect()
}

#[test]
fn the_released_pin_of_the_sample_repeats_the_check_in_the_lane_and_skips_the_group() -> TestResult
{
    let found = render(RELEASED, None)?;
    assert_eq!(
        counts(&found),
        [("rust-lane-local-mode-root", 1), ("rust-root", 1)]
    );
    Ok(())
}

#[test]
fn the_sample_at_0_1_112_runs_the_check_once_per_root_before_its_tests() -> TestResult {
    let found = render(NEXT, None)?;
    assert_eq!(counts(&found), [("rust-root", 1), ("rust-tools", 2)]);
    for (id, job) in &found {
        let test = job.ids.iter().position(|step| step == "test");
        for check in &job.checks {
            assert!(
                test.is_some_and(|test| *check < test),
                "{id}: {:?}",
                job.ids
            );
        }
    }
    Ok(())
}

#[test]
fn a_testless_group_of_the_sample_gets_its_checks_in_the_build_phase_only_from_0_1_112()
-> TestResult {
    let after = render(NEXT, Some("tools/"))?;
    assert_eq!(counts(&after), [("rust-root", 1), ("rust-tools", 2)]);
    let tools = &after["rust-tools"];
    assert!(
        !tools.ids.iter().any(|step| step == "test"),
        "{:?}",
        tools.ids
    );
    for check in &tools.checks {
        let wait = format!("steps.{}.outcome != 'failure'", tools.ids[*check]);
        assert!(
            tools.ids[check + 1].starts_with("compile")
                && tools.conditions[check + 1].contains(&wait),
            "{:?} {:?}",
            tools.ids,
            tools.conditions
        );
    }
    let before = render(RELEASED, Some("tools/"))?;
    assert_eq!(
        counts(&before),
        [("rust-lane-local-mode-root", 1), ("rust-root", 1)]
    );
    Ok(())
}
