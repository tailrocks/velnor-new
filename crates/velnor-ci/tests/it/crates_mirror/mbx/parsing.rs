// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::super::fixture::TestResult;
use super::scope::{assert_plain, assert_refused, declared_job, run_with_args};

#[test]
fn a_symlinked_directory_flag_loads_aliases_from_the_directory_it_resolves_to() -> TestResult {
    let repo = declared_job()?;
    let inside = repo.root().join("Cargo.toml").display().to_string();
    let home = repo.root().join("alias-home");
    fs::create_dir_all(home.join(".cargo"))?;
    fs::create_dir_all(home.join("real"))?;
    fs::write(
        home.join(".cargo/config.toml"),
        format!("[alias]\nx = \"build --manifest-path {inside}\"\n"),
    )?;
    let outside = repo.root().join("out");
    fs::create_dir_all(&outside)?;
    std::os::unix::fs::symlink(home.join("real"), outside.join("link"))?;
    let link = outside.join("link").display().to_string();
    let launch = repo.root().join("launch");
    assert_refused(&run_with_args(&repo, &launch, &["-C", &link, "x"])?);
    Ok(())
}

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
fn a_clustered_short_directory_flag_names_the_project_the_command_runs_in() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let declaring = format!("-qC{}", repo.root().display());
    let scratch = format!("-qC{}", repo.root().join("scratch-project").display());
    assert_refused(&run_with_args(&repo, &launch, &[&declaring, "build"])?);
    assert_plain(&run_with_args(&repo, &launch, &[&scratch, "build"])?);
    Ok(())
}
