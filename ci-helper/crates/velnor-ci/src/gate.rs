// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::github::{
    ApiCheckRun, ApiCheckRuns, ApiCompare, ApiRepo, GitHub, Repo, check_runs_path, compare_path,
    parse, repo_path,
};

pub const REQUIRED_CHECK: &str = "Required";
const CHECK_APP: &str = "github-actions";

pub fn release_gate(github: &dyn GitHub, repo: &Repo, sha: &str) -> Result<String, CiError> {
    if sha.len() != 40
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(CiError::new(format!(
            "`{sha}` is not a full lowercase commit SHA"
        )));
    }
    let ApiRepo { default_branch } = parse(&github.get(&repo_path(repo))?, "repository")?;
    let compare: ApiCompare = parse(
        &github.get(&compare_path(repo, &default_branch, sha))?,
        "comparison",
    )?;
    let checks: ApiCheckRuns = parse(
        &github.get(&check_runs_path(repo, sha, REQUIRED_CHECK))?,
        "check runs",
    )?;
    if checks.total_count > checks.check_runs.len() {
        return Err(CiError::new(format!(
            "GitHub lists {} runs of check {REQUIRED_CHECK} on commit {sha} and returned {}",
            checks.total_count,
            checks.check_runs.len()
        )));
    }
    let mut failures = Vec::new();
    if !matches!(compare.status.as_str(), "identical" | "behind") {
        failures.push(format!(
            "commit {sha} is not on the default branch {default_branch} (comparison: {})",
            compare.status
        ));
    }
    if let Err(reason) = required_passed(&checks.check_runs) {
        failures.push(format!(
            "check {REQUIRED_CHECK} did not pass on commit {sha}: {reason}"
        ));
    }
    if failures.is_empty() {
        Ok(format!(
            "commit {sha} is on {default_branch}\ncheck {REQUIRED_CHECK} passed on commit {sha}\n"
        ))
    } else {
        Err(CiError::new(failures.join("\n")))
    }
}

pub fn check_tag(version: &str, tag: &str) -> Result<String, CiError> {
    let expected = format!("rust-ci-v{version}");
    if tag == expected {
        Ok(format!("tag {tag} names version {version}\n"))
    } else {
        Err(CiError::new(format!(
            "tag {tag} must be {expected}, because this program is version {version}"
        )))
    }
}

fn required_passed(runs: &[ApiCheckRun]) -> Result<(), String> {
    let ours: Vec<&ApiCheckRun> = runs
        .iter()
        .filter(|run| {
            run.name == REQUIRED_CHECK && run.app.as_ref().is_some_and(|app| app.slug == CHECK_APP)
        })
        .collect();
    if ours.is_empty() {
        return Err(format!("no run of it by {CHECK_APP}"));
    }
    for run in ours {
        if run.status != "completed" {
            return Err(format!("it is {}", run.status));
        }
        match run.conclusion.as_deref() {
            Some("success") => {}
            other => {
                return Err(format!(
                    "it concluded {}",
                    other.unwrap_or("without a result")
                ));
            }
        }
    }
    Ok(())
}
