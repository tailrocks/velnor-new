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

/// Pin the push branch so plan/generate work without origin/HEAD.
fn pin_branch(repo: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let config = repo.join(".velnor").join("config.toml");
    let mut body = std::fs::read_to_string(&config)?;
    body.push_str("\n[workflow]\ndefault_branch = \"main\"\n");
    std::fs::write(&config, body)?;
    Ok(())
}

/// Recommendation bodies from the trailing plan-report section.
fn plan_recommendations(stdout: &str) -> Vec<String> {
    let mut in_section = false;
    let mut out = Vec::new();
    for line in stdout.lines() {
        if line == "Recommendations" {
            in_section = true;
        } else if in_section {
            if let Some(body) = line.strip_prefix("  ") {
                if body != "(none)" {
                    out.push(body.to_owned());
                }
            } else {
                break;
            }
        }
    }
    out
}

#[test]
fn plan_emits_recommendations_once_to_stdout_only() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-plan-once")?;
    git_init(&tmp)?;
    assert_eq!(code(&spawn(&["init"], &[], &tmp)?), 0);
    pin_branch(&tmp)?;
    let plan = spawn(&["plan"], &[], &tmp)?;
    assert_eq!(code(&plan), 0);
    let stdout = String::from_utf8_lossy(&plan.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&plan.stderr).into_owned();
    assert!(stdout.contains("Velnor Actions plan"));
    assert!(stderr.is_empty(), "plan stderr must stay empty: {stderr:?}");
    let recs = plan_recommendations(&stdout);
    assert!(!recs.is_empty());
    for rec in &recs {
        let hits = stdout.lines().filter(|line| line.trim() == rec).count()
            + stderr.lines().filter(|line| line.trim() == rec).count();
        assert_eq!(hits, 1, "recommendation emitted {hits}x: {rec}");
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn generate_keeps_recommendations_on_stderr() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-gen-recs")?;
    git_init(&tmp)?;
    assert_eq!(code(&spawn(&["init"], &[], &tmp)?), 0);
    pin_branch(&tmp)?;
    let plan = spawn(&["plan"], &[], &tmp)?;
    assert_eq!(code(&plan), 0);
    let expected = plan_recommendations(&String::from_utf8_lossy(&plan.stdout));
    assert!(!expected.is_empty());
    let outer = fresh_tempdir("smoke-gen-preview")?;
    let preview = outer.join("preview");
    let generated = spawn(
        &["generate", "--output-dir", preview.to_str().unwrap_or("/")],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&generated), 0);
    assert!(generated.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&generated.stderr).into_owned();
    for rec in &expected {
        assert!(
            stderr.lines().any(|line| line == rec),
            "generate stderr missing: {rec}"
        );
    }
    assert!(preview.join(".github/workflows/velnor.yml").is_file());
    cleanup(&tmp);
    cleanup(&outer);
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
