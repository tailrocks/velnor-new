// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_with_notes;
use velnor_model::RepoPath;

use crate::archive::jobs_of;
use crate::base_save::{names, step};
use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const PURGE: &str = "Delete the private-base caches unless the repository is private";
const GUARD: &str =
    "github.ref == format(''refs/heads/{0}'', github.event.repository.default_branch)";

fn rootless(version: &str) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut repo = view(&[], None)?;
    pin_helper(&mut repo, version)?;
    Ok(jobs_of(&workflow(&repo)?))
}

fn renders_a_workflow(version: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let mut repo = view(&[], None)?;
    pin_helper(&mut repo, version)?;
    let (files, _) = expected_with_notes(&repo)?;
    Ok(files.contains_key(&RepoPath::new(".github/workflows/ci.yml")?))
}

#[test]
fn a_repository_without_a_cargo_root_still_runs_the_purge_on_the_default_branch() -> TestResult {
    let jobs = rootless("0.1.112")?;
    let (_, trim) = jobs
        .iter()
        .find(|(id, _)| id == "rust-trim")
        .ok_or_else(|| {
            format!(
                "no trim job in {:?}",
                jobs.iter().map(|(id, _)| id).collect::<Vec<_>>()
            )
        })?;
    let purge = step(trim, PURGE).ok_or("no purge step")?;
    assert!(
        purge.contains("run: velnor-ci cache purge-private --if-public\n"),
        "{purge}"
    );
    assert!(trim.contains(GUARD), "{trim}");
    assert!(!trim.contains("needs:"), "{trim}");
    assert_eq!(
        names(trim).last().map(String::as_str),
        Some(PURGE),
        "{trim}"
    );
    Ok(())
}

#[test]
fn a_helper_that_cannot_purge_gets_no_job_in_a_repository_without_a_cargo_root() -> TestResult {
    assert!(!renders_a_workflow("0.1.111")?);
    Ok(())
}
