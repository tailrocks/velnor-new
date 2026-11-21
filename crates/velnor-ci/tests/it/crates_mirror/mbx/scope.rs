// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Output;

use crate::io::Repo;

use super::super::fixture::{TestResult, setup_command};
use super::{MBX_DECLARED, REFUSED, REGISTRY, configured_repo, plain_cargo, wrapper_command};

const MISSING: &str = "mise's cargo wrapper is missing after activation";

pub(super) fn declared_job() -> Result<Repo, Box<dyn std::error::Error>> {
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

pub(super) fn assert_plain(output: &Output) {
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
fn a_directory_outside_the_workspace_that_declares_mbx_still_fails_closed() -> TestResult {
    let repo = declared_job()?;
    let workspace = repo.root().join("workspace");
    fs::create_dir_all(&workspace)?;
    let output = run_in(&repo, repo.root(), Some(&workspace), false)?;
    assert_refused(&output);
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

pub(super) fn run_with_args(
    repo: &Repo,
    directory: &Path,
    args: &[&str],
) -> Result<Output, Box<dyn std::error::Error>> {
    let plain = plain_cargo(repo)?;
    fs::create_dir_all(directory)?;
    Ok(wrapper_command(repo.root(), &plain, false, &[])?
        .current_dir(directory)
        .env("VELNOR_CARGO_MBX", "required")
        .args(args)
        .output()?)
}

pub(super) fn assert_refused(output: &Output) {
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains(REFUSED));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("plain-cargo-ran"));
}

#[test]
fn every_manifest_path_spelling_decides_by_the_project_it_names() -> TestResult {
    let repo = declared_job()?;
    let scratch = repo.root().join("scratch/deeper");
    let launch = repo.root().join("launch");
    let inside = repo.root().join("Cargo.toml").display().to_string();
    let outside = repo
        .root()
        .join("scratch-project/Cargo.toml")
        .display()
        .to_string();
    for manifest in [&inside, &outside] {
        let spellings: [Vec<String>; 4] = [
            vec!["metadata".into(), "-m".into(), manifest.clone()],
            vec!["metadata".into(), format!("-m={manifest}")],
            vec!["metadata".into(), format!("-m{manifest}")],
            vec![
                "metadata".into(),
                "--manifest-path".into(),
                manifest.clone(),
            ],
        ];
        for spelling in &spellings {
            let args: Vec<&str> = spelling.iter().map(String::as_str).collect();
            if manifest == &inside {
                assert_refused(&run_with_args(&repo, &scratch, &args)?);
            } else {
                assert_plain(&run_with_args(&repo, &launch, &args)?);
            }
        }
    }
    Ok(())
}

#[test]
fn a_custom_subcommand_keeps_its_own_dash_m_and_stays_scoped_to_the_launching_directory()
-> TestResult {
    let repo = declared_job()?;
    let spec = repo.root().join("scratch-project/spec");
    let spec = spec.display().to_string();
    assert_refused(&run_with_args(&repo, repo.root(), &["foo", "-m", &spec])?);
    assert_refused(&run_with_args(
        &repo,
        repo.root(),
        &["foo", &format!("-m{spec}")],
    )?);
    let manifest = repo.root().join("scratch-project/Cargo.toml");
    let manifest = manifest.display().to_string();
    assert_plain(&run_with_args(
        &repo,
        repo.root(),
        &["foo", "--manifest-path", &manifest],
    )?);
    assert_plain(&run_with_args(
        &repo,
        repo.root(),
        &[
            "+nightly",
            "-Z",
            "unstable-options",
            "build",
            "-m",
            &manifest,
        ],
    )?);
    Ok(())
}

#[test]
fn an_install_from_a_path_is_scoped_by_the_crate_it_installs() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let inside = repo.root().display().to_string();
    let outside = repo.root().join("scratch-project").display().to_string();
    for spelling in [
        vec!["install".to_owned(), "--path".to_owned(), inside.clone()],
        vec!["install".to_owned(), format!("--path={inside}")],
    ] {
        let args: Vec<&str> = spelling.iter().map(String::as_str).collect();
        assert_refused(&run_with_args(&repo, &launch, &args)?);
    }
    assert_plain(&run_with_args(
        &repo,
        repo.root(),
        &["install", "--path", &outside],
    )?);
    assert_plain(&run_with_args(&repo, &launch, &["add", "--path", &inside])?);
    Ok(())
}

#[test]
fn a_workspace_path_that_cannot_be_resolved_fails_closed() -> TestResult {
    let repo = declared_job()?;
    let member = repo.root().join("crates/member");
    let gone = repo.root().join("renamed-checkout");
    assert_refused(&run_in(&repo, &member, Some(&gone), false)?);
    Ok(())
}

#[test]
fn a_workspace_path_that_is_a_regular_file_fails_closed() -> TestResult {
    let repo = declared_job()?;
    let member = repo.root().join("crates/member");
    let file = repo.root().join("not-a-directory");
    fs::write(&file, "")?;
    assert_refused(&run_in(&repo, &member, Some(&file), false)?);
    let output = run_in(&repo, &member, Some(&file), true)?;
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains(MISSING));
    Ok(())
}

#[test]
fn an_unreadable_or_invalid_config_counts_as_declaring_mbx() -> TestResult {
    let broken: [(&str, &[u8]); 3] = [
        ("invalid toml", b"[tools\n"),
        ("invalid utf-8", &[0xff, 0xfe, 0x00]),
        ("truncated mid-table", b"[wrappers.cargo]\ncommand = "),
    ];
    for (name, bytes) in broken {
        let repo = declared_job()?;
        fs::write(repo.root().join("mise.toml"), bytes)?;
        let output = run_in(&repo, repo.root(), None, false)?;
        assert!(!output.status.success(), "{name}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(REFUSED),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn a_config_that_is_a_directory_counts_as_declaring_mbx() -> TestResult {
    let repo = declared_job()?;
    fs::remove_file(repo.root().join("mise.toml"))?;
    fs::create_dir(repo.root().join("mise.toml"))?;
    assert_refused(&run_in(&repo, repo.root(), None, false)?);
    Ok(())
}

#[test]
fn a_broken_config_in_the_workspace_root_fails_closed_for_a_member_build() -> TestResult {
    let repo = declared_job()?;
    let workspace = repo.root().join("workspace");
    let member = workspace.join("crates/member");
    fs::create_dir_all(&member)?;
    fs::write(workspace.join("mise.toml"), "[tools\n")?;
    assert_refused(&run_in(&repo, &member, Some(&workspace), false)?);
    Ok(())
}

#[test]
fn a_directory_without_any_config_still_runs_plain() -> TestResult {
    let repo = declared_job()?;
    let fixture = repo.root().join("fixture-without-config");
    assert_plain(&run_in(&repo, &fixture, None, false)?);
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
