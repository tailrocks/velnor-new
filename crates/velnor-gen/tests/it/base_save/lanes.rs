// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn only_the_lint_and_test_lanes_save_and_the_sources_come_from_the_test_lane() -> TestResult {
    let all = jobs("0.1.81", "shape = \"lint-test\"\n")?;
    let saves: Vec<&str> = all
        .iter()
        .filter(|(_, block)| saving(block))
        .map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(saves, ["rust-lint", "rust-test"]);
    let lint = &all
        .iter()
        .find(|(id, _)| id == "rust-lint")
        .ok_or("lint")?
        .1;
    let test = &all
        .iter()
        .find(|(id, _)| id == "rust-test")
        .ok_or("test")?
        .1;
    assert!(step(lint, "Save the sources cache").is_none(), "{lint}");
    assert!(step(test, "Save the sources cache").is_some(), "{test}");
    assert!(step(lint, "Save the target cache").is_some());
    assert!(step(test, "Save the target cache").is_some());
    for (id, block) in &all {
        if !saving(block) {
            assert!(
                !block.contains("actions: write") || id == "rust-trim",
                "{id}"
            );
            assert!(!block.contains("Save the target cache"), "{id}");
        }
    }
    Ok(())
}

#[test]
fn the_archive_build_job_removes_the_archive_and_the_group_jobs_save_nothing() -> TestResult {
    let all = jobs("0.1.81", "shape = \"groups\"\n")?;
    let saves: Vec<&str> = all
        .iter()
        .filter(|(_, block)| saving(block))
        .map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(saves, ["rust-lint", "rust-test"]);
    let build = &all
        .iter()
        .find(|(id, _)| id == "rust-test")
        .ok_or("test")?
        .1;
    let decide = step(build, "Decide whether to save the build directory").ok_or("decide")?;
    assert!(
        decide.contains("--archive nextest-archive.tar.zst"),
        "{decide}"
    );
    let order = names(build);
    let upload = order
        .iter()
        .position(|n| n == "Upload the tests")
        .ok_or("upload")?;
    let save = order
        .iter()
        .position(|n| n == "Save the target cache")
        .ok_or("save")?;
    assert!(upload < save);
    Ok(())
}

#[test]
fn the_saved_lanes_build_command_equals_its_restore_time_command() -> TestResult {
    for (config, tested, lanes) in [
        ("", true, vec!["rust"]),
        (
            "shape = \"lint-test\"\n",
            true,
            vec!["rust-lint", "rust-test"],
        ),
        ("shape = \"groups\"\n", true, vec!["rust-lint", "rust-test"]),
        ("", false, vec!["rust"]),
    ] {
        let repo = with_library(config, tested)?;
        let jobs = jobs_of(&workflow(&repo)?);
        for id in lanes {
            let block = jobs
                .iter()
                .find(|(job, _)| job == id)
                .map(|(_, block)| block.as_str())
                .ok_or(id)?;
            let (saved, restored) = saved_and_restored(block)?;
            assert_eq!(saved, restored, "{config} tested={tested} {id}");
        }
    }
    Ok(())
}

#[test]
fn the_test_prebuild_ignores_a_nextest_default_filter() -> TestResult {
    let text = workflow(&pinned("0.1.81", "")?)?;
    let lane = jobs_of(&text)
        .into_iter()
        .find(|(id, _)| id == "rust")
        .map(|(_, block)| block)
        .ok_or("no lane")?;
    let build = step(&lane, "Build the tests").ok_or("no prebuild")?;
    let run = run_command(build).ok_or("no prebuild command")?;
    assert_eq!(
        run,
        "velnor-ci run -- velnor-ci inputs build -- cargo-nextest nextest run --workspace --locked --cargo-message-format=json-render-diagnostics --ignore-default-filter --no-run"
    );
    Ok(())
}

#[test]
fn the_saving_lane_builds_the_doctests_and_the_direct_harnesses_before_the_target_save()
-> TestResult {
    for config in ["", "shape = \"lint-test\"\n", "shape = \"groups\"\n"] {
        let lane = lane_building_doctests(config)?;
        let order = names(&lane);
        let save = order
            .iter()
            .position(|name| name == "Save the target cache")
            .ok_or("no target save")?;
        for built in ["Doctests", "Test package-0 custom"] {
            let index = order.iter().position(|name| name == built).ok_or(built)?;
            assert!(
                index < save,
                "{config}: {built} is built after the save: {order:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn the_lane_runs_the_nextest_run_before_the_direct_harnesses_and_the_doctests() -> TestResult {
    for config in ["", "shape = \"lint-test\"\n"] {
        let order = names(&lane_building_doctests(config)?);
        let run = order
            .iter()
            .position(|name| name == "Test")
            .ok_or("no nextest run")?;
        for built in ["Doctests", "Test package-0 custom"] {
            let index = order.iter().position(|name| name == built).ok_or(built)?;
            assert!(
                run < index,
                "{config}: the nextest run is after {built}: {order:?}"
            );
        }
    }
    Ok(())
}
