// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoView, Target, TargetKind, TargetName};
use velnor_rust::{LOCK_PATH, pin_lock};

use crate::archive::jobs_of;
use crate::facts::TestResult;
use crate::lock::{COMMIT_FILE, SUMS};
use crate::pipeline::{view, workflow};

const EVENTS: &str = "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch) && (github.event_name == ''push'' || github.event_name == ''schedule'' || github.event_name == ''workflow_dispatch'')";

pub(crate) fn pinned(version: &str, config: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    repo.tracked_paths.insert(LOCK_PATH);
    repo.files.insert(
        LOCK_PATH,
        pin_lock(version, COMMIT_FILE, SUMS)?.into_bytes(),
    );
    Ok(repo)
}

pub(crate) fn jobs(
    version: &str,
    config: &str,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    Ok(jobs_of(&workflow(&pinned(version, config)?)?))
}

pub(crate) fn steps(block: &str) -> Vec<&str> {
    block.split("\n      - ").skip(1).collect()
}

pub(crate) fn step<'a>(block: &'a str, name: &str) -> Option<&'a str> {
    steps(block).into_iter().find(|step| {
        step.lines()
            .any(|line| line.trim_start_matches("- ").trim() == format!("name: {name}"))
    })
}

pub(crate) fn names(block: &str) -> Vec<String> {
    steps(block)
        .iter()
        .filter_map(|step| {
            step.lines()
                .find_map(|line| line.trim().strip_prefix("name: ").map(str::to_owned))
        })
        .collect()
}

fn saving(block: &str) -> bool {
    block.contains("velnor-ci cache decide")
}

fn with_library(config: &str, tested: bool) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = pinned("0.1.81", config)?;
    for metadata in repo.rust_metadata.values_mut() {
        for member in &mut metadata.members {
            if !tested {
                member.targets.clear();
            }
            member.targets.push(Target {
                name: TargetName::new("lib")?,
                kinds: vec![TargetKind::Lib],
                tests: false,
                doctest: false,
                source: None,
                requires_features: false,
            });
        }
    }
    Ok(repo)
}

fn run_command(step: &str) -> Option<String> {
    let line = step.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("run: ")
            .or_else(|| trimmed.strip_prefix("run: '"))
    })?;
    Some(line.trim_end_matches('\'').replace("''", "'"))
}

fn compile_selection(step: &str) -> Option<String> {
    let run = run_command(step)?;
    let command = run.split("velnor-ci run -- ").nth(1).or_else(|| {
        run.split("velnor-ci run --require-doctests=$DOCTESTS_KNOWN -- ")
            .nth(1)
    })?;
    let command = command.trim_start_matches("velnor-ci inputs build -- ");
    if command.starts_with("cargo-fmt") {
        return None;
    }
    let compiles = command.starts_with("cargo-clippy")
        || command.starts_with("cargo-nextest")
        || command.starts_with("cargo ");
    if !compiles {
        return None;
    }
    if let Some(package) = command.split(" -p ").nth(1) {
        return Some(format!("-p {}", package.split_whitespace().next()?));
    }
    command
        .contains("--workspace")
        .then(|| "--workspace".to_owned())
}

fn saved_and_restored(block: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let steps = steps(block);
    let save = steps
        .iter()
        .position(|step| step.contains("name: Save the target cache"))
        .ok_or("no target save")?;
    let restore = steps
        .iter()
        .position(|step| step.contains("name: Restore the target cache"))
        .ok_or("no target restore")?;
    let saved = steps[..save]
        .iter()
        .find_map(|step| compile_selection(step).filter(|selection| selection == "--workspace"))
        .ok_or("no build before the save")?;
    let restored = steps[restore + 1..]
        .iter()
        .find_map(|step| compile_selection(step))
        .ok_or("no build after the restore")?;
    Ok((saved, restored))
}

#[test]
fn the_one_job_lane_saves_after_tests_and_input_audit_and_only_on_the_default_branch() -> TestResult
{
    let all = jobs("0.1.81", "")?;
    let (_, lane) = all
        .iter()
        .find(|(id, _)| id == "rust")
        .ok_or("no rust job")?;
    let order = names(lane);
    let build = order
        .iter()
        .position(|name| name == "Build the tests")
        .ok_or("no test build")?;
    let audit = order
        .iter()
        .position(|name| name == "velnor-ci inputs audit")
        .ok_or("no audit")?;
    let tested = order
        .iter()
        .position(|name| name == "Test")
        .ok_or("no test")?;
    assert!(build < tested && tested < audit);
    let saved: Vec<&str> = order[audit + 1..audit + 7]
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        saved,
        [
            "velnor-ci inputs save",
            "Decide whether to save the build directory",
            "Save the target cache",
            "Record the target cache key",
            "Save the sources cache",
            "Record the sources cache key",
        ]
    );
    for name in &saved {
        let text = step(lane, name).ok_or("missing step")?;
        assert!(text.contains(EVENTS), "{name}: {text}");
        assert!(text.contains("!cancelled()"), "{name}: {text}");
        assert!(!text.contains("pull_request"), "{name}: {text}");
    }
    let target = step(lane, "Save the target cache").ok_or("no target save")?;
    assert!(
        target.contains("steps.base.outputs.save == ''true''"),
        "{target}"
    );
    let sources = step(lane, "Save the sources cache").ok_or("no sources save")?;
    assert!(
        sources.contains("steps.sources_root.outputs.cache-hit != ''true''"),
        "{sources}"
    );
    assert!(
        sources.contains("steps.base.outputs.sources == ''true''"),
        "{sources}"
    );
    assert!(!lane.contains("actions: write"), "{lane}");
    let decide = step(lane, "Decide whether to save the build directory").ok_or("no decide")?;
    assert!(
        decide.contains("run: velnor-ci cache decide --target-dir target"),
        "{decide}"
    );
    Ok(())
}

#[test]
fn a_helper_older_than_0_1_81_gets_no_save_no_token_permission_and_no_age_limit() -> TestResult {
    let text = workflow(&pinned("0.1.79", "full_run = \"off\"\n")?)?;
    for absent in [
        "cache decide",
        "cache trim",
        "inputs save",
        "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9\n        with:\n          path: target",
        "actions: write",
        "VELNOR_BASE_MAX_AGE_DAYS",
    ] {
        assert!(!text.contains(absent), "{absent}");
    }
    Ok(())
}

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
        "velnor-ci run -- velnor-ci inputs build -- cargo-nextest nextest run --workspace --locked --ignore-default-filter --no-run"
    );
    Ok(())
}
