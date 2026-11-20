// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use velnor_local::io::local_ci::{pin_branch, pin_workflows};
use velnor_local::timeout::TimeoutScale;

use crate::local_ci::{ELF_ARM64, binary, parsed};
use crate::support::temp_dir;

const SAMPLE: &str = "../../generator/samples/helper-lock/expected/.github/workflows/rust-ci.yml";
const WORKFLOW: &str = ".github/workflows/rust-ci.yml";
const LINUX_ARM64: &str = "runner.arch == ''ARM64'' && ''";
const REFUSAL: &str = "velnor-ci integrity check failed";

fn sample() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SAMPLE);
    std::fs::read_to_string(path).unwrap_or_else(|e| unreachable!("{e}"))
}

fn without_cache_mode(text: &str) -> String {
    text.split_inclusive('\n')
        .filter(|line| !line.starts_with("    cache-mode:"))
        .collect()
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-C", &repo.display().to_string()])
        .args(args)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn checkout(name: &str) -> PathBuf {
    let repo = temp_dir(name);
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(repo.join(".github/workflows")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(repo.join(WORKFLOW), sample()).unwrap_or_else(|e| unreachable!("{e}"));
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "workflow",
        ],
    );
    repo
}

fn pinned_digest(workflow: &str) -> String {
    let at = workflow
        .find(LINUX_ARM64)
        .unwrap_or_else(|| unreachable!("no linux-arm64 pin"))
        + LINUX_ARM64.len();
    workflow[at..at + 64].to_owned()
}

fn launcher(workflow: &str) -> String {
    let prefix = "VELNOR_HELPER_LAUNCHER: ";
    let line = workflow
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix))
        .map(|value| {
            value.strip_prefix('&').map_or(value, |anchor| {
                anchor.split_once(' ').map_or(anchor, |(_, rest)| rest)
            })
        })
        .filter(|value| value.starts_with('\''))
        .unwrap_or_else(|| unreachable!("no launcher"));
    line.trim_matches('\'').replace("''", "'")
}

fn checksum_program() -> (&'static str, &'static str) {
    for program in ["/usr/bin/sha256sum", "/bin/sha256sum"] {
        if Path::new(program).exists() {
            return (program, "--check --status");
        }
    }
    ("/usr/bin/shasum", "-a 256 --check --status")
}

fn launch(workflow: &str, pin: &str, helper: &Path) -> Output {
    let temp = temp_dir("dev-binary-launch");
    let (checksum, arguments) = checksum_program();
    Command::new("bash")
        .args(["-c", &launcher(workflow), "velnor-ci"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("RUNNER_TEMP", &temp)
        .env("VELNOR_HELPER", helper)
        .env("VELNOR_HELPER_SHA256", pin)
        .env("VELNOR_HELPER_MKTEMP", "/usr/bin/mktemp")
        .env("VELNOR_HELPER_CP", "/bin/cp")
        .env("VELNOR_HELPER_CHMOD", "/bin/chmod")
        .env("VELNOR_HELPER_ENV", "/usr/bin/env")
        .env("VELNOR_HELPER_CHECKSUM", checksum)
        .env("VELNOR_HELPER_CHECKSUM_ARGUMENTS", arguments)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn refused(output: &Output) -> bool {
    String::from_utf8_lossy(&output.stderr).contains(REFUSAL)
}

#[tokio::test]
async fn a_local_dev_binary_is_accepted_by_the_launcher_check_with_the_flag() {
    let repo = checkout("dev-binary-accepted");
    let file = binary(&temp_dir("dev-binary-file"), &ELF_ARM64);
    let flag = file.display().to_string();
    let args = parsed(&repo, "plain", &["--velnor-ci-binary", &flag]);
    pin_workflows(&args, &repo, "t", TimeoutScale::default())
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let workflow =
        std::fs::read_to_string(repo.join(WORKFLOW)).unwrap_or_else(|e| unreachable!("{e}"));
    let output = launch(&workflow, &pinned_digest(&workflow), &file);
    assert!(!refused(&output), "{output:?}");
    assert_eq!(
        git(&repo, &["log", "-1", "--format=%s"]),
        "velnor-local: rewrite the workflows of this run"
    );
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
}

#[tokio::test]
async fn without_the_flag_a_local_binary_is_still_refused_and_the_pin_is_untouched() {
    let repo = checkout("dev-binary-refused");
    let file = binary(&temp_dir("dev-binary-file-2"), &ELF_ARM64);
    let args = parsed(&repo, "plain", &[]);
    pin_workflows(&args, &repo, "t", TimeoutScale::default())
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let workflow =
        std::fs::read_to_string(repo.join(WORKFLOW)).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(workflow, without_cache_mode(&sample()));
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "2");
    let output = launch(&workflow, &pinned_digest(&workflow), &file);
    assert!(refused(&output), "{output:?}");
}

#[tokio::test]
async fn the_flag_changes_only_the_linux_arm64_digest_and_the_check_still_refuses_any_other_binary()
{
    let repo = checkout("dev-binary-only-pin");
    let file = binary(&temp_dir("dev-binary-file-3"), &ELF_ARM64);
    let other = binary(&temp_dir("dev-binary-other"), b"another build");
    let flag = file.display().to_string();
    let args = parsed(&repo, "plain", &["--velnor-ci-binary", &flag]);
    pin_workflows(&args, &repo, "t", TimeoutScale::default())
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let workflow =
        std::fs::read_to_string(repo.join(WORKFLOW)).unwrap_or_else(|e| unreachable!("{e}"));
    let original = without_cache_mode(&sample());
    assert_ne!(workflow, original);
    assert_eq!(
        workflow.replace(&pinned_digest(&workflow), &pinned_digest(&original)),
        original
    );
    let output = launch(&workflow, &pinned_digest(&workflow), &other);
    assert!(refused(&output), "{output:?}");
}

#[tokio::test]
async fn a_file_that_is_not_a_linux_arm64_binary_is_refused_and_pair_mode_pins_like_plain() {
    let repo = checkout("dev-binary-bad");
    let good = binary(&temp_dir("dev-binary-file-4"), &ELF_ARM64);
    let text = binary(&temp_dir("dev-binary-text"), b"#!/bin/sh\n");
    let flag = text.display().to_string();
    let args = parsed(&repo, "plain", &["--velnor-ci-binary", &flag]);
    assert!(
        pin_workflows(&args, &repo, "t", TimeoutScale::default())
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(repo.join(WORKFLOW)).ok(),
        Some(sample())
    );
    let flag = good.display().to_string();
    let args = parsed(&repo, "pair", &["--velnor-ci-binary", &flag]);
    let pin = pin_workflows(&args, &repo, "t", TimeoutScale::default()).await;
    assert!(matches!(pin, Ok(Some(_))), "{pin:?}");
    assert_ne!(
        std::fs::read_to_string(repo.join(WORKFLOW)).ok(),
        Some(sample())
    );
}

#[tokio::test]
async fn a_branch_that_already_exists_in_the_checkout_is_pinned_when_the_driver_switches_to_it() {
    let repo = checkout("dev-binary-branch");
    git(&repo, &["branch", "feature"]);
    let file = binary(&temp_dir("dev-binary-file-5"), &ELF_ARM64);
    let flag = file.display().to_string();
    let args = parsed(&repo, "plain", &["--velnor-ci-binary", &flag]);
    let pin = pin_workflows(&args, &repo, "t", TimeoutScale::default())
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .unwrap_or_else(|| unreachable!("no pin"));
    git(&repo, &["switch", "-q", "feature"]);
    let workflow =
        std::fs::read_to_string(repo.join(WORKFLOW)).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(refused(&launch(
        &workflow,
        &pinned_digest(&workflow),
        &file
    )));

    pin_branch(&repo, "t", Some(&pin), TimeoutScale::default())
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));

    let workflow =
        std::fs::read_to_string(repo.join(WORKFLOW)).unwrap_or_else(|e| unreachable!("{e}"));
    let output = launch(&workflow, &pinned_digest(&workflow), &file);
    assert!(!refused(&output), "{output:?}");
    let commits = git(&repo, &["rev-list", "--count", "HEAD"]);
    pin_branch(&repo, "t", Some(&pin), TimeoutScale::default())
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), commits);
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
}

fn outside(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    std::fs::write(dir.join("rust-ci.yml"), sample()).unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

async fn refusal(repo: &Path, name: &str) -> String {
    let file = binary(&temp_dir(name), &ELF_ARM64);
    let flag = file.display().to_string();
    let args = parsed(repo, "plain", &["--velnor-ci-binary", &flag]);
    match pin_workflows(&args, repo, "t", TimeoutScale::default()).await {
        Ok(_) => unreachable!("a symlinked workflow path was accepted"),
        Err(error) => error.to_string(),
    }
}

fn unchanged(victim: &Path) {
    assert_eq!(
        std::fs::read_to_string(victim).ok(),
        Some(sample()),
        "{} was written through a symlink",
        victim.display()
    );
}

fn link(target: &Path, at: &Path) {
    std::os::unix::fs::symlink(target, at).unwrap_or_else(|e| unreachable!("{e}"));
}

#[tokio::test]
async fn a_symlinked_workflow_file_is_refused_and_its_target_outside_the_copy_is_left_alone() {
    let repo = checkout("dev-binary-link-file");
    let elsewhere = outside("dev-binary-victim-file");
    let victim = elsewhere.join("rust-ci.yml");
    let at = repo.join(".github/workflows/linked.yml");
    link(&victim, &at);
    let message = refusal(&repo, "dev-binary-link-file-bin").await;
    assert!(message.contains(&at.display().to_string()), "{message}");
    unchanged(&victim);
}

#[tokio::test]
async fn a_symlinked_workflows_directory_is_refused_and_its_target_is_left_alone() {
    let repo = checkout("dev-binary-link-workflows");
    let elsewhere = outside("dev-binary-victim-workflows");
    let at = repo.join(".github/workflows");
    std::fs::remove_dir_all(&at).unwrap_or_else(|e| unreachable!("{e}"));
    link(&elsewhere, &at);
    let message = refusal(&repo, "dev-binary-link-workflows-bin").await;
    assert!(message.contains(&at.display().to_string()), "{message}");
    unchanged(&elsewhere.join("rust-ci.yml"));
}

#[tokio::test]
async fn a_symlinked_github_directory_is_refused_and_its_target_is_left_alone() {
    let repo = checkout("dev-binary-link-github");
    let elsewhere = temp_dir("dev-binary-victim-github");
    std::fs::create_dir_all(elsewhere.join("workflows")).unwrap_or_else(|e| unreachable!("{e}"));
    let victim = elsewhere.join("workflows/rust-ci.yml");
    std::fs::write(&victim, sample()).unwrap_or_else(|e| unreachable!("{e}"));
    let at = repo.join(".github");
    std::fs::remove_dir_all(&at).unwrap_or_else(|e| unreachable!("{e}"));
    link(&elsewhere, &at);
    let message = refusal(&repo, "dev-binary-link-github-bin").await;
    assert!(message.contains(&at.display().to_string()), "{message}");
    unchanged(&victim);
}

fn sources(root: &Path, skip: &[PathBuf], found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "target" || name == ".git" || name == "node_modules" || skip.contains(&path) {
            continue;
        }
        if path.is_dir() {
            sources(&path, skip, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") || name == "Cargo.toml" {
            found.push(path);
        }
    }
}

#[test]
fn nothing_outside_velnor_local_can_reach_the_override() {
    let root = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut files = Vec::new();
    // generator/samples holds recorded copies of the repo, not live code.
    let skip = [
        root.join("crates/velnor-local"),
        root.join("generator/samples"),
    ];
    sources(&root, &skip, &mut files);
    assert!(files.len() > 100, "{}", files.len());
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let depends = text
            .lines()
            .any(|line| line.trim_start().starts_with("velnor-local ="));
        assert!(!depends, "{} depends on velnor-local", file.display());
        for needle in ["velnor_local::", "--velnor-ci-binary", "pin_workflows"] {
            assert!(
                !text.contains(needle),
                "{} mentions {needle}: the production launcher must not be able to reach the dev-binary override",
                file.display()
            );
        }
    }
}
