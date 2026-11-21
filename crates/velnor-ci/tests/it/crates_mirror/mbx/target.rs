// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::fixture::TestResult;
use super::scope::{assert_plain, assert_refused, declared_job, run_with_args};

#[test]
fn a_directory_flag_names_the_project_the_command_runs_in() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let declaring = repo.root().display().to_string();
    let scratch = repo.root().join("scratch-project").display().to_string();
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["-C", &declaring, "build"],
    )?);
    assert_plain(&run_with_args(&repo, &launch, &["-C", &scratch, "build"])?);
    Ok(())
}

#[test]
fn a_direct_script_target_is_scoped_by_its_path() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let inside = repo.root().join("tool.rs").display().to_string();
    let outside = repo
        .root()
        .join("scratch-project/tool.rs")
        .display()
        .to_string();
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", &inside],
    )?);
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", &outside],
    )?);
    Ok(())
}

#[test]
fn an_attached_directory_flag_names_the_project_the_command_runs_in() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let declaring = format!("-C{}", repo.root().display());
    let scratch = format!("-C{}", repo.root().join("scratch-project").display());
    assert_refused(&run_with_args(&repo, &launch, &[&declaring, "build"])?);
    assert_plain(&run_with_args(&repo, &launch, &[&scratch, "build"])?);
    Ok(())
}

#[test]
fn each_directory_flag_is_applied_in_order_like_cargo() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let scratch = repo.root().join("scratch-project").display().to_string();
    let declaring = repo.root().display().to_string();
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &[&format!("-C{scratch}"), "-C", &declaring, "build"],
    )?);
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &["-C", &declaring, &format!("-C{scratch}"), "build"],
    )?);
    Ok(())
}
