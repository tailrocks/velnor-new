// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A helper that records the build in its test step lets the saves wait for that
//! record, so a saving lane needs no separate `--no-run` build step before its tests.

use crate::base_save::{jobs, names, step};
use crate::facts::TestResult;

const SAVES: &[&str] = &[
    "velnor-ci inputs save",
    "Decide whether to save the build directory",
    "Save the target cache",
    "Record the target cache key",
    "Save the sources cache",
    "Record the sources cache key",
];

const SHAPES: &[&str] = &["one-job", "lint-test", "groups"];

/// The jobs that save a target cache and run tests, as (id, rendered block).
fn testing_savers(
    version: &str,
    config: &str,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    Ok(jobs(version, config)?
        .into_iter()
        .filter(|(_, lane)| {
            lane.contains("Save the target cache") && names(lane).iter().any(|name| name == "Test")
        })
        .collect())
}

fn no_run_step(lane: &str) -> Option<&str> {
    crate::base_save::steps(lane)
        .into_iter()
        .find(|step| step.contains("--no-run"))
}

#[test]
fn a_saving_run_has_no_separate_no_run_build_step_when_the_helper_records_the_build() -> TestResult
{
    for shape in SHAPES.iter().copied() {
        let config = format!("shape = {shape:?}\non_main = 'everything'\n");
        for (id, lane) in testing_savers("0.1.107", &config)? {
            assert!(
                no_run_step(&lane).is_none(),
                "{shape} {id}: {:?}",
                names(&lane)
            );
        }
    }
    Ok(())
}

#[test]
fn every_save_waits_for_the_test_step_to_record_a_build() -> TestResult {
    for shape in SHAPES.iter().copied() {
        let config = format!("shape = {shape:?}\non_main = 'everything'\n");
        for (id, lane) in testing_savers("0.1.107", &config)? {
            for name in SAVES.iter().copied() {
                let Some(saved) = step(&lane, name) else {
                    continue;
                };
                assert!(
                    saved.contains("outputs.built == ''true''"),
                    "{shape} {id} {name}: {saved}"
                );
                assert!(!saved.contains("build_1"), "{shape} {id} {name}: {saved}");
            }
        }
    }
    Ok(())
}

#[test]
fn every_recorded_build_names_a_test_step_that_exists() -> TestResult {
    for shape in SHAPES.iter().copied() {
        let config = format!("shape = {shape:?}\non_main = 'everything'\n");
        for (id, lane) in testing_savers("0.1.107", &config)? {
            for (index, _) in lane.match_indices("outputs.built ==") {
                let start =
                    lane[..index].rfind("steps.").ok_or("no step reference")? + "steps.".len();
                let gate: String = lane[start..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                assert!(
                    lane.contains(&format!("id: {gate}\n")),
                    "{shape} {id}: {gate}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn a_helper_without_the_record_keeps_the_separate_build_step() -> TestResult {
    for (id, lane) in testing_savers("0.1.106", "")? {
        assert!(no_run_step(&lane).is_some(), "{id}: {:?}", names(&lane));
    }
    Ok(())
}
