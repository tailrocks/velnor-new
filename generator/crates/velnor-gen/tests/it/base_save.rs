// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoView;
use velnor_rust::{LOCK_PATH, pin_lock};

use crate::archive::jobs_of;
use crate::facts::TestResult;
use crate::lock::{COMMIT_FILE, SUMS};
use crate::pipeline::{view, workflow};

const EVENTS: &str = "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch) && (github.event_name == ''push'' || github.event_name == ''schedule'' || github.event_name == ''workflow_dispatch'')";

fn pinned(version: &str, config: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    repo.tracked_paths.insert(LOCK_PATH);
    repo.files.insert(
        LOCK_PATH,
        pin_lock(version, COMMIT_FILE, SUMS)?.into_bytes(),
    );
    Ok(repo)
}

fn jobs(version: &str, config: &str) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    Ok(jobs_of(&workflow(&pinned(version, config)?)?))
}

fn steps(block: &str) -> Vec<&str> {
    block.split("\n      - ").skip(1).collect()
}

fn step<'a>(block: &'a str, name: &str) -> Option<&'a str> {
    steps(block).into_iter().find(|step| {
        step.lines()
            .any(|line| line.trim_start_matches("- ").trim() == format!("name: {name}"))
    })
}

fn names(block: &str) -> Vec<String> {
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

#[test]
fn the_one_job_lane_saves_after_its_last_cargo_step_in_this_order_and_only_on_the_default_branch()
-> TestResult {
    let all = jobs("0.1.81", "")?;
    let (_, lane) = all
        .iter()
        .find(|(id, _)| id == "rust")
        .ok_or("no rust job")?;
    let order = names(lane);
    let last_cargo = order
        .iter()
        .rposition(|name| name == "Check app alone" || name == "Documentation")
        .ok_or("no doc step")?;
    let tail: Vec<&str> = order[last_cargo + 1..].iter().map(String::as_str).collect();
    assert_eq!(
        &tail[..7],
        [
            "velnor-ci inputs save",
            "Decide whether to save the build directory",
            "Save the target cache",
            "Enforce the repository cache budget",
            "Delete the older target entries",
            "Save the sources cache",
            "Enforce the repository cache budget",
        ]
    );
    for name in tail[..7]
        .iter()
        .filter(|name| **name != "Enforce the repository cache budget")
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
    assert!(!sources.contains("steps.base"), "{sources}");
    assert!(lane.contains("      actions: write\n"), "{lane}");
    let decide = step(lane, "Decide whether to save the build directory").ok_or("no decide")?;
    assert!(
        decide.contains("run: velnor-ci cache decide --target-dir target"),
        "{decide}"
    );
    Ok(())
}

#[test]
fn the_trim_step_deletes_by_the_family_and_compat_prefix_with_a_token() -> TestResult {
    let all = jobs("0.1.81", "")?;
    let (_, lane) = all
        .iter()
        .find(|(id, _)| id == "rust")
        .ok_or("no rust job")?;
    let trim = step(lane, "Delete the older target entries").ok_or("no trim")?;
    assert!(
        trim.contains("CACHE_PREFIX: 'velnor-tgt-rust-root-${{ runner.os }}-${{ runner.arch }}-${{ steps.prepare.outputs.compat }}-'"),
        "{trim}"
    );
    assert!(
        trim.contains("GITHUB_TOKEN: '${{ github.token }}'"),
        "{trim}"
    );
    assert!(
        trim.contains("velnor-ci cache trim --prefix \"$CACHE_PREFIX\""),
        "{trim}"
    );
    Ok(())
}

#[test]
fn a_helper_older_than_0_1_81_gets_no_save_no_token_permission_and_no_age_limit() -> TestResult {
    let text = workflow(&pinned("0.1.79", "")?)?;
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
        if !saving(block) && id != "rust-cache-budget" {
            assert!(!block.contains("actions: write"), "{id}");
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
fn restored_carries_the_age_limit_on_the_events_that_build_a_clean_base() -> TestResult {
    let limit = |config: &str| -> Result<String, Box<dyn std::error::Error>> {
        let all = jobs("0.1.81", config)?;
        let (_, lane) = all
            .iter()
            .find(|(id, _)| id == "rust")
            .ok_or("no rust job")?;
        let restored = step(lane, "velnor-ci restored").ok_or("no restored")?;
        Ok(restored
            .lines()
            .find(|line| line.contains("VELNOR_BASE_MAX_AGE_DAYS"))
            .unwrap_or("")
            .trim()
            .to_owned())
    };
    let ref_is_main =
        "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch)";
    assert_eq!(
        limit("")?,
        format!(
            "VELNOR_BASE_MAX_AGE_DAYS: '${{{{ {ref_is_main} && github.event_name != ''push'' && ''7'' || ''0'' }}}}'"
        )
    );
    let every_run =
        format!("VELNOR_BASE_MAX_AGE_DAYS: '${{{{ {ref_is_main} && ''7'' || ''0'' }}}}'");
    assert_eq!(limit("on_main = \"everything\"\n")?, every_run);
    assert_eq!(limit("full_run = \"off\"\n")?, every_run);
    Ok(())
}
