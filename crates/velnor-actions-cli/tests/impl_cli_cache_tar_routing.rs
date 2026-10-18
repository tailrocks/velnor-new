//! Binary routing tests for the cache tar adapter aliases.

#![cfg(unix)]

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir};

const OP: &str = "cache-action-stage-tar-v1";

#[test]
fn normal_binary_name_keeps_the_public_cli_route() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
        .arg("--version")
        .env("VELNOR_INTERNAL_OP", OP)
        .output()?;

    assert_eq!(code(&output), 0, "{:?}", output.stderr);
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("velnor-actions "));
    Ok(())
}

#[test]
fn tar_aliases_keep_version_available_without_workflow_context() -> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("cache-tar-version")?;
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"));

    for alias in ["tar", "gtar"] {
        let path = temp.join(alias);
        symlink(&binary, &path)?;
        let output = Command::new(&path).arg("--version").env_clear().output()?;
        assert_eq!(code(&output), 0, "alias={alias}: {:?}", output.stderr);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "tar (GNU tar) velnor-cache-adapter-v1\n"
        );
    }

    cleanup(&temp);
    Ok(())
}

#[test]
fn tar_archive_route_requires_workflow_context_and_valid_stage_inputs() -> Result<(), Box<dyn Error>>
{
    let temp = fresh_tempdir("cache-tar-gate")?;
    let temp = fs::canonicalize(temp)?;
    let path = temp.join("tar");
    symlink(env!("CARGO_BIN_EXE_velnor-actions"), &path)?;

    let missing_run = run(
        &path,
        &temp,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_INTERNAL_OP", OP),
        ],
    )?;
    assert_ne!(code(&missing_run), 0);
    assert!(
        String::from_utf8_lossy(&missing_run.stderr).contains("cache_tar_runner_context_invalid")
    );

    let outside_actions = run(
        &path,
        &temp,
        &[
            ("GITHUB_ACTIONS", "false"),
            ("GITHUB_RUN_ID", "12"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_INTERNAL_OP", OP),
        ],
    )?;
    assert_ne!(code(&outside_actions), 0);
    assert!(
        String::from_utf8_lossy(&outside_actions.stderr)
            .contains("cache_tar_runner_context_invalid")
    );

    let wrong_operation = run(
        &path,
        &temp,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_RUN_ID", "12"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_INTERNAL_OP", "normal-tools-cache-stage-path-v1"),
        ],
    )?;
    assert_ne!(code(&wrong_operation), 0);
    assert!(
        String::from_utf8_lossy(&wrong_operation.stderr)
            .contains("cache_tar_runner_context_invalid")
    );

    let missing_stage = run(
        &path,
        &temp,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_RUN_ID", "12"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_INTERNAL_OP", OP),
        ],
    )?;
    assert_ne!(code(&missing_stage), 0);
    assert!(String::from_utf8_lossy(&missing_stage.stderr).contains("cache_stage_input_missing"));

    cleanup(&temp);
    Ok(())
}

fn run(alias: &Path, runner_temp: &Path, envs: &[(&str, &str)]) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(alias);
    command
        .args(["-tf", "cache.tar"])
        .env_clear()
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_WORKSPACE", runner_temp)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    for (name, value) in envs {
        command.env(name, value);
    }
    Ok(command.output()?)
}
