// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::super::fixture::TestResult;
use super::scope::{assert_plain, assert_refused, declared_job, run_with_args};

#[test]
fn a_trailing_zscript_flag_after_an_external_rs_subcommand_is_its_own_argument() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let inside = repo.root().join("Cargo.toml").display().to_string();
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["tool.rs", "--manifest-path", &inside, "-Zscript"],
    )?);
    Ok(())
}

#[test]
fn a_clustered_zscript_flag_scopes_the_script_it_names_alone() -> TestResult {
    let repo = declared_job()?;
    fs::create_dir_all(repo.root().join("scratch-project"))?;
    let outside = repo
        .root()
        .join("scratch-project/tool.rs")
        .display()
        .to_string();
    assert_plain(&run_with_args(
        &repo,
        repo.root(),
        &["-qZscript", &outside],
    )?);
    assert_plain(&run_with_args(
        &repo,
        repo.root(),
        &["-q", "-Z", "script", &outside],
    )?);
    Ok(())
}

#[test]
fn an_equals_attached_zscript_flag_scopes_the_script_it_names_alone() -> TestResult {
    let repo = declared_job()?;
    fs::create_dir_all(repo.root().join("scratch-project"))?;
    let outside = repo
        .root()
        .join("scratch-project/tool.rs")
        .display()
        .to_string();
    assert_plain(&run_with_args(
        &repo,
        repo.root(),
        &["-Z=script", &outside],
    )?);
    Ok(())
}

#[test]
fn a_leading_option_separator_does_not_hide_the_subcommand_or_its_manifest() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let inside = repo.root().join("Cargo.toml").display().to_string();
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["--", "foo", "--manifest-path", &inside],
    )?);
    Ok(())
}

#[test]
fn a_clustered_short_manifest_flag_names_the_project_the_command_runs_in() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let inside = repo.root().join("Cargo.toml").display().to_string();
    let outside = repo
        .root()
        .join("scratch-project/Cargo.toml")
        .display()
        .to_string();
    assert_refused(&run_with_args(&repo, &launch, &["build", "-qm", &inside])?);
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["build", &format!("-qm{inside}")],
    )?);
    assert_plain(&run_with_args(&repo, &launch, &["build", "-qm", &outside])?);
    Ok(())
}
