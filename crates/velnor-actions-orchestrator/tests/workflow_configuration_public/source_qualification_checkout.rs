// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn git(dir: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!("git {} failed in {}", args.join(" "), dir.display()).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn rev_parse(dir: &Path, rev: &str) -> Option<String> {
    Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", rev])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[test]
fn source_qualification_verify_fetches_absent_pr_base() -> Result<(), Box<dyn Error>> {
    let sandbox = tempfile::tempdir()?;
    let origin = sandbox.path().join("origin");
    std::fs::create_dir_all(&origin)?;
    git(&origin, &["init", "-b", "main", "."])?;
    std::fs::write(origin.join("base.txt"), "base\n")?;
    git(&origin, &["add", "base.txt"])?;
    git(&origin, &["commit", "-m", "base"])?;
    git(&origin, &["checkout", "-b", "feature"])?;
    std::fs::write(origin.join("head.txt"), "head\n")?;
    git(&origin, &["add", "head.txt"])?;
    git(&origin, &["commit", "-m", "head"])?;
    git(&origin, &["checkout", "main"])?;
    git(&origin, &["merge", "--no-ff", "feature", "-m", "merge"])?;
    let merged = git(&origin, &["rev-parse", "HEAD"])?;
    let base = git(&origin, &["rev-parse", "HEAD^1"])?;
    let head = git(&origin, &["rev-parse", "HEAD^2"])?;

    let clone = sandbox.path().join("clone");
    std::fs::create_dir_all(&clone)?;
    git(&clone, &["init", "-b", "main", "."])?;
    git(
        &clone,
        &[
            "remote",
            "add",
            "origin",
            origin.to_str().ok_or("origin path")?,
        ],
    )?;
    git(
        &clone,
        &["fetch", "--no-tags", "--depth", "1", "origin", &merged],
    )?;
    git(&clone, &["checkout", "--detach", &merged])?;
    if rev_parse(&clone, &format!("{base}^{{commit}}")).is_some() {
        return Err("shallow fixture unexpectedly contains the base commit".into());
    }

    let runner_temp = sandbox.path().join("runner-temp");
    std::fs::create_dir_all(runner_temp.join("source-qualification-artifacts"))?;
    let script = workspace_root().join("scripts/generator-release/source-qualification.sh");
    let status = Command::new("bash")
        .arg(&script)
        .arg("verify-exact-clean-source-checkout")
        .current_dir(&clone)
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_SHA", &merged)
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_REF", "refs/pull/407/merge")
        .env(
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/source-qualification.yml@refs/heads/main",
        )
        .env("GITHUB_WORKFLOW_SHA", &merged)
        .env("RUNNER_OS", "macOS")
        .env("RUNNER_ARCH", "ARM64")
        .env("GITHUB_WORKSPACE", &clone)
        .env("PR_HEAD_SHA", &head)
        .env("PR_BASE_SHA", &base)
        .env("RUNNER_TEMP", &runner_temp)
        .status()?;
    if !status.success() {
        return Err("verify phase failed with the base commit outside the shallow fetch".into());
    }
    if rev_parse(&clone, &format!("{base}^{{commit}}")).is_none() {
        return Err("verify phase passed without materializing the base commit".into());
    }
    Ok(())
}
