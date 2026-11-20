// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The target and sources saves wait for the compile steps, not for the tests.

use crate::base_save::{jobs, names, step};
use crate::facts::TestResult;

const SAVES: [&str; 6] = [
    "velnor-ci inputs save",
    "Decide whether to save the build directory",
    "Save the target cache",
    "Record the target cache key",
    "Save the sources cache",
    "Record the sources cache key",
];

fn rust_lane() -> Result<String, Box<dyn std::error::Error>> {
    jobs("0.1.81", "")?
        .into_iter()
        .find(|(id, _)| id == "rust")
        .map(|(_, lane)| lane)
        .ok_or_else(|| "no rust job".into())
}

fn saving_steps(lane: &str) -> Result<Vec<&str>, Box<dyn std::error::Error>> {
    SAVES
        .iter()
        .map(|name| step(lane, name).ok_or_else(|| format!("no {name}").into()))
        .collect()
}

#[test]
fn a_save_is_not_gated_on_every_step_of_the_job_succeeding() -> TestResult {
    let lane = rust_lane()?;
    for saved in saving_steps(&lane)? {
        assert!(!saved.contains("success()"), "{saved}");
    }
    Ok(())
}

#[test]
fn a_save_requires_the_step_that_builds_the_tests_to_succeed() -> TestResult {
    let lane = rust_lane()?;
    let build = step(&lane, "Build the tests").ok_or("no build")?;
    assert!(build.contains("id: build_1"), "{build}");
    for saved in saving_steps(&lane)? {
        assert!(
            saved.contains("steps.build_1.outcome == ''success''"),
            "{saved}"
        );
    }
    Ok(())
}

#[test]
fn a_save_never_follows_a_cancelled_run() -> TestResult {
    let lane = rust_lane()?;
    for saved in saving_steps(&lane)? {
        assert!(saved.contains("!cancelled()"), "{saved}");
    }
    Ok(())
}

#[test]
fn a_failed_input_audit_still_blocks_the_save() -> TestResult {
    let lane = rust_lane()?;
    let audit = step(&lane, "velnor-ci inputs audit").ok_or("no audit")?;
    assert!(audit.contains("id: audit_1"), "{audit}");
    for saved in saving_steps(&lane)? {
        assert!(
            saved.contains("steps.audit_1.outcome != ''failure''"),
            "{saved}"
        );
    }
    Ok(())
}

#[test]
fn the_trusted_default_branch_gate_is_unchanged() -> TestResult {
    let lane = rust_lane()?;
    let events = "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch) && (github.event_name == ''push'' || github.event_name == ''schedule'' || github.event_name == ''workflow_dispatch'')";
    for name in &SAVES[..5] {
        let saved = step(&lane, name).ok_or("missing")?;
        assert!(saved.contains(events), "{name}: {saved}");
        assert!(!saved.contains("pull_request"), "{name}: {saved}");
    }
    let target = step(&lane, "Save the target cache").ok_or("target")?;
    assert!(
        target.contains("steps.base.outputs.save == ''true''"),
        "{target}"
    );
    Ok(())
}

#[test]
fn the_tools_and_delta_saves_keep_their_rules() -> TestResult {
    let lane = rust_lane()?;
    for name in names(&lane) {
        let delta = name.contains("delta") || name.contains("tools");
        let Some(saved) = step(&lane, &name).filter(|_| delta) else {
            continue;
        };
        assert!(!saved.contains("build_1"), "{name}: {saved}");
        assert!(!saved.contains("audit_1"), "{name}: {saved}");
    }
    Ok(())
}

#[test]
fn a_target_saved_after_a_failed_test_says_so() -> TestResult {
    let lane = rust_lane()?;
    let note = step(&lane, "Note the target cache saved after a test failure").ok_or("no note")?;
    assert!(
        note.contains("steps.save_target_root.outcome == ''success''"),
        "{note}"
    );
    assert!(note.contains("steps.test.outcome == ''failure''"), "{note}");
    assert!(note.contains("!cancelled()"), "{note}");
    assert!(
        note.contains("target cache saved after test failure"),
        "{note}"
    );
    let order = names(&lane);
    let record = order
        .iter()
        .position(|name| name == "Record the target cache key");
    let noted = order
        .iter()
        .position(|name| name == "Note the target cache saved after a test failure");
    assert!(record < noted);
    Ok(())
}

#[test]
fn every_gate_names_a_step_that_exists_in_every_shape() -> TestResult {
    for shape in ["one-job", "lint-test", "groups"] {
        let config = format!("shape = {shape:?}\non_main = 'everything'\n");
        for (id, lane) in jobs("0.1.81", &config)? {
            for (index, _) in lane.match_indices("steps.build_") {
                let gate: String = lane[index + "steps.".len()..]
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
