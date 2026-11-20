// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Output;

use crate::io::Repo;

use super::super::fixture::{TestResult, setup_command};
use super::{MBX_DECLARED, REFUSED, REGISTRY, configured_repo, plain_cargo, wrapper_command};

const MISSING: &str = "mise's cargo wrapper is missing after activation";

fn declared_job() -> Result<Repo, Box<dyn std::error::Error>> {
    let repo = configured_repo("crates-mirror-mbx-scope")?;
    fs::write(repo.root().join("mise.toml"), MBX_DECLARED)?;
    let setup = setup_command(repo.root(), REGISTRY)
        .current_dir(repo.root())
        .output()?;
    assert!(setup.status.success(), "{setup:?}");
    let mode = fs::read_to_string(repo.root().join("velnor-cargo/velnor-cargo-mode"))?;
    assert!(mode.contains("mbx=required"), "{mode}");
    Ok(repo)
}

fn run_in(
    repo: &Repo,
    directory: &Path,
    workspace: Option<&Path>,
    activated: bool,
) -> Result<Output, Box<dyn std::error::Error>> {
    let plain = plain_cargo(repo)?;
    let data = repo.root().join("activated-data");
    fs::create_dir_all(&data)?;
    fs::create_dir_all(directory)?;
    let mut command = wrapper_command(repo.root(), &plain, false, &[])?;
    command
        .current_dir(directory)
        .env("VELNOR_CARGO_MBX", "required");
    if activated {
        command
            .env("VELNOR_CARGO_MISE_PHASE", "activated")
            .env("VELNOR_CARGO_MISE_DATA", &data);
    }
    if let Some(workspace) = workspace {
        command.env("GITHUB_WORKSPACE", workspace);
    }
    Ok(command.output()?)
}

fn assert_plain(output: &Output) {
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
}

#[test]
fn a_test_spawned_cargo_in_a_directory_that_declares_nothing_runs_plain_under_the_job_mode()
-> TestResult {
    let repo = declared_job()?;
    let fixture = repo.root().join("fixture-outside-the-workspace");
    assert_plain(&run_in(&repo, &fixture, None, false)?);
    assert_plain(&run_in(&repo, &fixture, None, true)?);
    Ok(())
}

#[test]
fn a_test_fixture_outside_the_workspace_runs_plain_even_beside_a_declaring_workspace() -> TestResult
{
    let repo = declared_job()?;
    let fixture = repo.root().join("fixture-outside-the-workspace");
    let workspace = repo.root().join("workspace");
    fs::create_dir_all(&workspace)?;
    fs::write(workspace.join("mise.toml"), MBX_DECLARED)?;
    assert_plain(&run_in(&repo, &fixture, Some(&workspace), true)?);
    Ok(())
}

#[test]
fn a_build_in_the_declaring_directory_still_fails_closed_without_the_mise_wrapper() -> TestResult {
    let repo = declared_job()?;
    let output = run_in(&repo, repo.root(), None, true)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains(MISSING), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
    let output = run_in(&repo, repo.root(), None, false)?;
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains(REFUSED));
    Ok(())
}

#[test]
fn a_build_in_a_workspace_subdirectory_still_fails_closed_when_the_workspace_declares_mbx()
-> TestResult {
    let repo = declared_job()?;
    let workspace = repo.root().join("workspace");
    let member = workspace.join("crates/member");
    fs::create_dir_all(&member)?;
    fs::write(workspace.join("mise.toml"), MBX_DECLARED)?;
    let output = run_in(&repo, &member, Some(&workspace), true)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains(MISSING), "{stderr}");
    Ok(())
}

fn run_with_manifest(
    repo: &Repo,
    directory: &Path,
    manifest: &str,
) -> Result<Output, Box<dyn std::error::Error>> {
    let plain = plain_cargo(repo)?;
    fs::create_dir_all(directory)?;
    Ok(wrapper_command(repo.root(), &plain, false, &[])?
        .current_dir(directory)
        .env("VELNOR_CARGO_MBX", "required")
        .args(["metadata", manifest])
        .output()?)
}

#[test]
fn the_project_named_by_manifest_path_decides_not_the_launching_directory() -> TestResult {
    let repo = declared_job()?;
    let outside = repo.root().join("scratch-project");
    fs::create_dir_all(&outside)?;
    let manifest = outside.join("Cargo.toml").display().to_string();
    let output = run_with_manifest(&repo, repo.root(), &format!("--manifest-path={manifest}"))?;
    assert_plain(&output);
    Ok(())
}

#[test]
fn a_manifest_path_inside_the_declaring_directory_still_fails_closed() -> TestResult {
    let repo = declared_job()?;
    let scratch = repo.root().join("scratch");
    let manifest = repo.root().join("Cargo.toml").display().to_string();
    let output = run_with_manifest(&repo, &scratch, &format!("--manifest-path={manifest}"))?;
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains(REFUSED));
    Ok(())
}

#[test]
fn a_cargo_started_by_a_running_cargo_or_test_runs_plain_but_a_step_cargo_fails_closed()
-> TestResult {
    let repo = declared_job()?;
    let plain = plain_cargo(&repo)?;
    let run = |nested: bool| -> Result<Output, Box<dyn std::error::Error>> {
        let mut command = wrapper_command(repo.root(), &plain, false, &[])?;
        command
            .current_dir(repo.root())
            .env("VELNOR_CARGO_MBX", "required");
        if nested {
            command.env("CARGO_MANIFEST_DIR", repo.root());
        }
        Ok(command.output()?)
    };
    assert_plain(&run(true)?);
    let output = run(false)?;
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains(REFUSED));
    Ok(())
}
