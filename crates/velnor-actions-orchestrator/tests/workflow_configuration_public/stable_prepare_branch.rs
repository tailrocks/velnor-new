// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::non_inputs_checkout_support::git_output;
use super::non_inputs_release::{jobs_of, release_workflows};
use super::scoped_workspace::TestResult;
use super::workflow_configuration_fixture::git;

#[test]
fn stable_prepare_puts_the_exact_source_on_the_default_branch_before_release_plz() -> TestResult {
    let workflows = release_workflows(None)?;
    let (_, workflow) = workflows
        .iter()
        .find(|(name, _)| name.as_str() == "binary-release.yml")
        .ok_or("binary-release.yml is missing")?;
    let (_, steps) = jobs_of(workflow)
        .into_iter()
        .find(|(job, _)| job == "stable-prepare")
        .ok_or("stable-prepare is missing")?;
    let position = |needle: &str| {
        steps
            .iter()
            .position(|step| step.contains(needle))
            .ok_or_else(|| format!("no step contains {needle}"))
    };
    let checkout = position("uses: actions/checkout@")?;
    let branch = position("id: source-branch")?;
    let plz = position("Prepare the version PR with qualified release-plz")?;
    assert!(checkout < branch && branch < plz, "step order is wrong");
    let value_of = |step: &str, key: &str| -> Result<String, Box<dyn Error>> {
        let line = step
            .lines()
            .find_map(|line| line.trim_start().strip_prefix(key))
            .ok_or_else(|| format!("step lacks {key}"))?;
        Ok(line.trim().to_owned())
    };
    let step = &steps[branch];
    let script: String = serde_json::from_str(&value_of(step, "run:")?)?;
    let default_branch = value_of(step, "VELNOR_DEFAULT_BRANCH:")?;
    assert_eq!(default_branch, "main");
    assert_eq!(
        value_of(step, "VELNOR_SOURCE_SHA:")?,
        value_of(&steps[checkout], "ref:")?
    );

    let repo = tempfile::tempdir()?;
    let root = repo.path();
    git(&["init", "-q", "-b", "main"], root)?;
    git(&["commit", "-q", "--allow-empty", "-m", "first"], root)?;
    git(&["commit", "-q", "--allow-empty", "-m", "second"], root)?;
    let first = git_output(root, &["rev-parse", "HEAD~1"])?
        .trim()
        .to_owned();
    let second = git_output(root, &["rev-parse", "HEAD"])?.trim().to_owned();
    git(&["branch", "-f", "stale", &first], root)?;
    git(&["checkout", "-q", "--detach", &second], root)?;
    git(&["branch", "-D", "main"], root)?;
    git(&["branch", "main", &first], root)?;

    let run = |sha: &str| {
        std::process::Command::new("bash")
            .arg("-c")
            .arg(&script)
            .current_dir(root)
            .env_clear()
            .envs(std::env::vars_os().filter(|(key, _)| key == "PATH"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("VELNOR_DEFAULT_BRANCH", &default_branch)
            .env("VELNOR_SOURCE_SHA", sha)
            .output()
    };
    let output = run(&second)?;
    assert!(
        output.status.success(),
        "source-branch step failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git_output(root, &["symbolic-ref", "--short", "HEAD"])?.trim(),
        "main"
    );
    assert_eq!(git_output(root, &["rev-parse", "HEAD"])?.trim(), second);
    assert_eq!(git_output(root, &["rev-parse", "main"])?.trim(), second);

    let missing = "0".repeat(40);
    assert!(!run(&missing)?.status.success());
    Ok(())
}
