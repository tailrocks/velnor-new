//! Binary-spawn smoke tests: init lifecycle, usage codes, env parity.

use std::error::Error;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, git_init, spawn};

#[test]
fn init_creates_config_then_refuses_overwrite() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-init")?;
    git_init(&tmp)?;
    let config = tmp.join(".velnor").join("config.toml");
    let first = spawn(&["init"], &[], &tmp)?;
    assert_eq!(code(&first), 0);
    assert!(config.is_file());
    let body = std::fs::read_to_string(&config)?;
    assert!(body.contains("schema = 1"));
    let second = spawn(&["init"], &[], &tmp)?;
    assert_eq!(code(&second), 1);
    assert_eq!(std::fs::read_to_string(&config)?, body);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn init_outside_work_tree_exits_one() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-norepo")?;
    let output = spawn(&["init"], &[], &tmp)?;
    assert_eq!(code(&output), 1);
    assert!(!tmp.join(".velnor").exists());
    cleanup(&tmp);
    Ok(())
}

#[test]
fn unknown_command_exits_two() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-unknown")?;
    let output = spawn(&["bogus"], &[], &tmp)?;
    assert_eq!(code(&output), 2);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn env_without_request_file_exits_two() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-env")?;
    git_init(&tmp)?;
    let runner = tmp.join("runner-temp");
    std::fs::create_dir_all(&runner)?;
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command
        .current_dir(&tmp)
        .env("VELNOR_INTERNAL_OP", "plan-v1")
        .env("RUNNER_TEMP", &runner)
        .env("VELNOR_RUN_KEY", "smoke");
    let gated = command.output()?;
    assert_eq!(code(&gated), 2);
    let bare = spawn(&[], &[], &tmp)?;
    assert_eq!(gated.stdout, bare.stdout);
    assert_eq!(gated.stderr, bare.stderr);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn help_is_identical_with_and_without_env() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-help")?;
    let plain = spawn(&["--help"], &[], &tmp)?;
    assert_eq!(code(&plain), 0);
    let runner = tmp.join("runner-temp");
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command
        .arg("--help")
        .current_dir(&tmp)
        .env("VELNOR_INTERNAL_OP", "merge-v1")
        .env("RUNNER_TEMP", &runner)
        .env("VELNOR_RUN_KEY", "smoke");
    let gated = command.output()?;
    assert_eq!(code(&gated), 0);
    assert_eq!(gated.stdout, plain.stdout);
    assert_eq!(gated.stderr, plain.stderr);
    cleanup(&tmp);
    Ok(())
}
