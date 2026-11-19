// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_files};
use velnor_model::RepoPath;
use velnor_rust::DetectError;

use crate::archive::jobs_of;
use crate::events::DENY_MISE;
use crate::facts::{TestResult, lock_pinned_tools};
use crate::pipeline::{view, workflow};

const KEY: &str = "          key: 'velnor-tools-${{ runner.os }}-${{ runner.arch }}-mise2026.11.10-${{ hashFiles(''mise.lock'') }}'\n";
const SAVE_IF: &str = "        if: '${{ github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch) && steps.tools.outputs.cache-hit != ''true'' }}'\n";

fn names(job: &str) -> Vec<&str> {
    job.lines()
        .filter_map(|line| {
            line.strip_prefix("      - name: ")
                .or_else(|| line.strip_prefix("        name: "))
        })
        .collect()
}

fn position(job: &str, name: &str) -> Option<usize> {
    names(job).iter().position(|found| *found == name)
}

fn job<'t>(jobs: &'t [(String, String)], id: &str) -> Result<&'t str, Box<dyn std::error::Error>> {
    jobs.iter()
        .find(|(found, _)| found == id)
        .map(|(_, text)| text.as_str())
        .ok_or_else(|| format!("no job {id}").into())
}

#[test]
fn a_lane_restores_the_tools_before_mise_runs_and_saves_only_a_default_branch_miss() -> TestResult {
    let jobs = jobs_of(&workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?);
    let lane = job(&jobs, "rust-lab")?;
    let restore = position(lane, "Restore the tools cache").ok_or("no tools restore")?;
    let install = position(lane, "Install tools").ok_or("no install")?;
    let save = position(lane, "Save the tools cache").ok_or("no tools save")?;
    let prepare = position(lane, "velnor-ci prepare").ok_or("no prepare")?;
    let targets = position(lane, "Add Rust targets");
    assert_eq!(install, restore + 1, "{lane}");
    assert!(save > install && save < prepare, "{lane}");
    assert!(targets.is_none_or(|targets| save > targets), "{lane}");
    assert_eq!(lane.matches(KEY).count(), 2, "{lane}");
    assert_eq!(lane.matches(SAVE_IF).count(), 1, "{lane}");
    assert!(lane.contains("      - id: tools\n        name: Restore the tools cache\n"));
    assert_eq!(lane.matches("velnor-tools-").count(), 2, "{lane}");
    for path in ["~/.local/share/mise", "~/.rustup"] {
        assert_eq!(lane.matches(&format!("            {path}\n")).count(), 2);
    }
    Ok(())
}

#[test]
fn a_shard_restores_the_tools_for_nextest_and_never_saves_a_partial_install() -> TestResult {
    let jobs = jobs_of(&workflow(&view(
        &["Cargo.toml", "lab/Cargo.toml"],
        Some(b"shape = \"groups\"\n"),
    )?)?);
    let shard = job(&jobs, "rust-test-default-root")?;
    let restore = position(shard, "Restore the tools cache").ok_or("no tools restore")?;
    assert_eq!(
        position(shard, "Install tools"),
        Some(restore + 1),
        "{shard}"
    );
    assert!(shard.contains("install_args: 'aqua:nextest-rs/nextest/cargo-nextest'"));
    assert!(!shard.contains("Save the tools cache"), "{shard}");
    let build = job(&jobs, "rust-test-root")?;
    assert!(build.contains("Save the tools cache"), "{build}");
    Ok(())
}

#[test]
fn the_plan_job_neither_restores_nor_saves_the_tools() -> TestResult {
    let jobs = jobs_of(&workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?);
    let plan = job(&jobs, "rust-plan")?;
    assert!(
        !plan.contains("tools cache") && !plan.contains("Install tools"),
        "{plan}"
    );
    Ok(())
}

#[test]
fn the_policy_job_restores_the_tools_and_saves_what_it_installed() -> TestResult {
    let mut policy = view(&["Cargo.toml"], None)?;
    for (path, text) in [("deny.toml", ""), ("mise.toml", DENY_MISE)] {
        let path = RepoPath::new(path)?;
        policy.tracked_paths.insert(path.clone());
        policy.files.insert(path, text.as_bytes().to_vec());
    }
    let jobs = jobs_of(&workflow(&policy)?);
    let lane = job(&jobs, "policy")?;
    let restore = position(lane, "Restore the tools cache").ok_or("no tools restore")?;
    assert_eq!(position(lane, "Install tools"), Some(restore + 1), "{lane}");
    assert_eq!(
        position(lane, "Save the tools cache"),
        Some(restore + 2),
        "{lane}"
    );
    Ok(())
}

#[test]
fn only_mise_lock_moves_the_key_never_a_comment_in_mise_toml() -> TestResult {
    let plain = view(&["Cargo.toml"], None)?;
    let before = workflow(&plain)?;
    let mut commented = plain.clone();
    let mise = RepoPath::new("mise.toml")?;
    let mut text = b"# bump when needed\n".to_vec();
    text.extend(commented.files.get(&mise).cloned().unwrap_or_default());
    commented.files.insert(mise, text);
    assert_eq!(workflow(&commented)?, before);
    assert!(before.contains("hashFiles(''mise.lock'')"));
    assert!(!before.contains("hashFiles(''mise.toml'')"));
    Ok(())
}

type Generated = Result<Result<(), GenerateError>, Box<dyn std::error::Error>>;

fn generate(
    mutate: impl FnOnce(&mut velnor_model::RepoView) -> TestResult,
    config: Option<&[u8]>,
) -> Generated {
    let mut repo = view(&["Cargo.toml"], config)?;
    mutate(&mut repo)?;
    Ok(expected_files(&repo).map(|_| ()))
}

fn set(repo: &mut velnor_model::RepoView, path: &str, text: &str) -> TestResult {
    let path = RepoPath::new(path)?;
    repo.tracked_paths.insert(path.clone());
    repo.files.insert(path, text.as_bytes().to_vec());
    Ok(())
}

#[test]
fn a_tool_without_a_prebuilt_binary_for_the_runner_fails_generation_naming_the_tool() -> TestResult
{
    let source_built = |repo: &mut velnor_model::RepoView| {
        set(
            repo,
            "mise.toml",
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\"cargo:ripgrep\" = \"14\"\n",
        )?;
        set(
            repo,
            "mise.lock",
            "[[tools.rust]]\nversion = \"1.99.1\"\n\n[[tools.\"cargo:ripgrep\"]]\nversion = \"14.0.0\"\n\n[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"0.9.151\"\n\n[tools.\"aqua:nextest-rs/nextest/cargo-nextest\".\"platforms.linux-x64\"]\nurl = \"https://example.invalid/nextest\"\n",
        )
    };
    assert_eq!(
        generate(source_built, None)?,
        Err(GenerateError::Detect(DetectError::ToolNotPrebuilt {
            tool: "cargo:ripgrep".to_owned(),
            platform: "linux-x64".to_owned()
        }))
    );
    let missing_platform = |repo: &mut velnor_model::RepoView| {
        lock_pinned_tools(repo)?;
        Ok(())
    };
    assert!(generate(missing_platform, None)?.is_ok());
    let arm = generate(missing_platform, Some(b"runner = \"ubuntu-24.04-arm\"\n"))?;
    assert_eq!(
        arm,
        Err(GenerateError::Detect(DetectError::ToolNotPrebuilt {
            tool: "aqua:nextest-rs/nextest/cargo-nextest".to_owned(),
            platform: "linux-arm64".to_owned()
        }))
    );
    Ok(())
}

#[test]
fn a_repository_with_tools_but_no_mise_lock_is_refused() -> TestResult {
    let without_lock = |repo: &mut velnor_model::RepoView| {
        repo.files.remove(&RepoPath::new("mise.lock")?);
        repo.tracked_paths.remove(&RepoPath::new("mise.lock")?);
        Ok(())
    };
    assert_eq!(
        generate(without_lock, None)?,
        Err(GenerateError::Detect(DetectError::MiseLockRequired {
            path: RepoPath::from_static("mise.lock")
        }))
    );
    Ok(())
}
