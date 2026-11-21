// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::super::fixture::TestResult;
use super::scope::{assert_plain, assert_refused, declared_job, run_with_args};
use crate::io::Repo;

fn aliases(launch: &Path, body: &str) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(launch.join(".cargo"))?;
    fs::write(launch.join(".cargo/config.toml"), body)?;
    Ok(())
}

fn manifests(repo: &Repo) -> (String, String) {
    let inside = repo.root().join("Cargo.toml").display().to_string();
    let outside = repo
        .root()
        .join("scratch-project/Cargo.toml")
        .display()
        .to_string();
    (inside, outside)
}

#[test]
fn the_built_in_aliases_and_read_manifest_are_scoped_by_the_project_they_name() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    for command in ["b", "c", "d", "t", "r", "rm", "read-manifest"] {
        assert_refused(&run_with_args(&repo, &launch, &[command, "-m", &inside])?);
        assert_refused(&run_with_args(
            &repo,
            &launch,
            &[command, &format!("-m{inside}")],
        )?);
        assert_plain(&run_with_args(&repo, &launch, &[command, "-m", &outside])?);
    }
    Ok(())
}

#[test]
fn a_user_alias_is_scoped_by_the_project_its_expansion_names() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    aliases(
        &launch,
        "[alias]\nxb = \"build --release\"\nxc = [\"check\", \"--all-targets\"]\nchained = \"xb\"\n",
    )?;
    let (inside, outside) = manifests(&repo);
    for alias in ["xb", "xc", "chained"] {
        assert_refused(&run_with_args(&repo, &launch, &[alias, "-m", &inside])?);
        assert_refused(&run_with_args(
            &repo,
            &launch,
            &[alias, "--manifest-path", &inside],
        )?);
        assert_plain(&run_with_args(&repo, &launch, &[alias, "-m", &outside])?);
    }
    Ok(())
}

#[test]
fn a_manifest_path_inside_an_alias_expansion_decides_the_project() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    aliases(
        &launch,
        &format!(
            "[alias]\nin = [\"build\", \"-m\", \"{inside}\"]\nout = [\"build\", \"-m\", \"{outside}\"]\n"
        ),
    )?;
    assert_refused(&run_with_args(&repo, &launch, &["in"])?);
    assert_plain(&run_with_args(&repo, &launch, &["out"])?);
    Ok(())
}

#[test]
fn an_alias_to_a_custom_subcommand_keeps_that_subcommands_own_flags() -> TestResult {
    let repo = declared_job()?;
    aliases(repo.root(), "[alias]\nnt = \"nextest run\"\n")?;
    let (_, outside) = manifests(&repo);
    assert_refused(&run_with_args(&repo, repo.root(), &["nt", "-m", &outside])?);
    assert_plain(&run_with_args(
        &repo,
        repo.root(),
        &["nt", "--manifest-path", &outside],
    )?);
    Ok(())
}

#[test]
fn an_alias_definition_that_cannot_be_read_fails_closed_for_an_unknown_subcommand() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, outside) = manifests(&repo);
    for body in ["[alias\n", "include = [\"other.toml\"]\n"] {
        aliases(&launch, body)?;
        for flag in ["-m", "--manifest-path"] {
            assert_refused(&run_with_args(&repo, &launch, &["xb", flag, &outside])?);
            assert_refused(&run_with_args(&repo, &launch, &["xb", flag, &inside])?);
        }
        assert_plain(&run_with_args(
            &repo,
            &launch,
            &["metadata", "-m", &outside],
        )?);
    }
    Ok(())
}

#[test]
fn a_legacy_cargo_config_alias_wins_over_config_toml() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, _) = manifests(&repo);
    aliases(&launch, "[alias]\nxb = \"check\"\n")?;
    fs::write(
        launch.join(".cargo/config"),
        format!("[alias]\nxb = \"build --manifest-path {inside}\"\n"),
    )?;
    assert_refused(&run_with_args(&repo, &launch, &["xb"])?);
    Ok(())
}

#[test]
fn an_alias_only_in_config_toml_is_ignored_when_a_legacy_config_exists() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (inside, _) = manifests(&repo);
    aliases(
        &launch,
        &format!("[alias]\nxb = \"build --manifest-path {inside}\"\n"),
    )?;
    fs::write(launch.join(".cargo/config"), "[alias]\nother = \"check\"\n")?;
    assert_plain(&run_with_args(&repo, &launch, &["xb"])?);
    Ok(())
}

#[test]
fn an_alias_given_on_the_command_line_fails_closed() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let (_, outside) = manifests(&repo);
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["--config", "alias.xb='build'", "xb", "-m", &outside],
    )?);
    Ok(())
}

#[test]
fn a_legacy_config_in_a_parent_directory_does_not_hide_a_child_config_toml_alias() -> TestResult {
    let repo = declared_job()?;
    let parent = repo.root().join("parent");
    let child = parent.join("child");
    let (inside, _) = manifests(&repo);
    fs::create_dir_all(parent.join(".cargo"))?;
    fs::write(parent.join(".cargo/config"), "[alias]\nother = \"check\"\n")?;
    aliases(
        &child,
        &format!("[alias]\nxb = \"build --manifest-path {inside}\"\n"),
    )?;
    assert_refused(&run_with_args(&repo, &child, &["xb"])?);
    Ok(())
}
