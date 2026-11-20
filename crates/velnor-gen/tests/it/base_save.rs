// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView, Target, TargetKind, TargetName};
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
        pin_lock(version, COMMIT_FILE, SUMS, None)?.into_bytes(),
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

fn with_doctests_and_harness(config: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = pinned("0.1.81", config)?;
    repo.files.insert(
        RepoPath::new("Cargo.toml")?,
        b"[package]\nname = \"package-0\"\n[[test]]\nname = \"custom\"\nharness = false\n".to_vec(),
    );
    for metadata in repo.rust_metadata.values_mut() {
        for member in &mut metadata.members {
            member.targets.push(Target {
                name: TargetName::new("lib")?,
                kinds: vec![TargetKind::Lib],
                tests: false,
                doctest: true,
                source: None,
                requires_features: false,
            });
            member.targets.push(Target {
                name: TargetName::new("custom")?,
                kinds: vec![TargetKind::Test],
                tests: true,
                doctest: false,
                source: None,
                requires_features: false,
            });
        }
    }
    Ok(repo)
}

fn lane_building_doctests(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = with_doctests_and_harness(config)?;
    jobs_of(&workflow(&repo)?)
        .into_iter()
        .find(|(_, block)| block.contains("name: Doctests"))
        .map(|(_, block)| block)
        .ok_or_else(|| "no lane builds the doctests".into())
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
    let saved: Vec<&str> = order[audit + 1..audit + 9]
        .iter()
        .filter_map(|name| (!name.starts_with("Fetch")).then_some(name.as_str()))
        .collect();
    assert_eq!(
        saved,
        [
            "velnor-ci inputs save",
            "Decide whether to save the build directory",
            "Save the target cache",
            "Record the target cache key",
            "Note the target cache saved after a test failure",
            "Save the sources cache",
            "Record the sources cache key",
        ]
    );
    for name in saved
        .iter()
        .filter(|name| !name.starts_with("Note the target"))
    {
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

mod lanes;
