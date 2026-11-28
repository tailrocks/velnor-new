// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::exec_builder::{job, run, run_message};
use crate::exec_support::{Scratch, host_os, run_fixture};

#[test]
fn exec_hello_job_succeeds() -> Result<()> {
    let ran = run_fixture("hello", &Scratch::new()?)?;
    assert_eq!(
        ran.stdout,
        "step 1 start: Say hello\nhello\nstep 1 succeeded\njob succeeded\n"
    );
    assert_eq!(ran.code, Some(0));
    Ok(())
}

#[test]
fn exec_reports_the_operating_system_of_the_host() -> Result<()> {
    let scratch = Scratch::new()?;
    let message = job(vec![run("os", "echo \"os=$RUNNER_OS\"")]);

    let ran = run_message(&scratch, &message)?;

    assert!(ran.has_line(&format!("os={}", host_os())), "{}", ran.stdout);
    Ok(())
}

#[test]
fn exec_outputs_flow_between_steps_and_job_outputs() -> Result<()> {
    let ran = run_fixture("outputs", &Scratch::new()?)?;
    assert!(ran.has_line("got-1"), "{}", ran.stdout);
    assert_eq!(
        ran.lines().get(ran.lines().len() - 2),
        Some(&"output out=1")
    );
    assert!(ran.has_line("job succeeded"));
    assert_eq!(ran.code, Some(0));
    Ok(())
}

#[test]
fn exec_github_env_and_path_apply_to_later_steps() -> Result<()> {
    let ran = run_fixture("env-path", &Scratch::new()?)?;
    assert!(ran.has_line("bar"), "{}", ran.stdout);
    assert!(ran.has_line("mytool-ran"), "{}", ran.stdout);
    assert!(ran.has_line("job succeeded"));
    assert_eq!(ran.code, Some(0));
    Ok(())
}

#[test]
fn exec_continue_on_error_and_always_steps() -> Result<()> {
    let ran = run_fixture("continue-on-error", &Scratch::new()?)?;
    for expected in [
        "step 1 failed",
        "after",
        "step 3 failed",
        "cleanup failure/success",
        "step 4 succeeded",
        "step 5 skipped",
        "job failed",
    ] {
        assert!(ran.has_line(expected), "{expected}\n{}", ran.stdout);
    }
    assert!(!ran.has_line("never"));
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn exec_step_timeout_fails_step() -> Result<()> {
    let ran = run_fixture("timeout", &Scratch::new()?)?;
    for expected in [
        "step 1 timed out after 0.02 minutes",
        "step 1 failed",
        "job failed",
    ] {
        assert!(ran.has_line(expected), "{expected}\n{}", ran.stdout);
    }
    assert!(ran.elapsed.as_secs() < 10, "{:?}", ran.elapsed);
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn exec_unsupported_uses_fails_job_before_any_step() -> Result<()> {
    let ran = run_fixture("unsupported-uses", &Scratch::new()?)?;
    assert_eq!(ran.stdout, "unsupported: uses actions/foo@v1\njob failed\n");
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn exec_unsupported_shell_fails_job() -> Result<()> {
    let ran = run_fixture("unsupported-shell", &Scratch::new()?)?;
    assert_eq!(ran.stdout, "unsupported: shell pwsh {0}\njob failed\n");
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn exec_unsupported_expression_function_fails_step() -> Result<()> {
    let ran = run_fixture("unsupported-expression", &Scratch::new()?)?;
    assert!(
        ran.has_line("unsupported expression function toYAML"),
        "{}",
        ran.stdout
    );
    assert!(ran.has_line("step 1 failed"));
    assert!(ran.has_line("job failed"));
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[test]
fn exec_masks_secret_values_in_logs() -> Result<()> {
    let ran = run_fixture("secret-mask", &Scratch::new()?)?;
    for secret in [
        "s3cr3t-value",
        "ghs_tokenvalue123",
        "hint-value-xyz",
        "late-secret-1",
    ] {
        assert!(!ran.stdout.contains(secret), "{secret}\n{}", ran.stdout);
    }
    assert_eq!(ran.lines().iter().filter(|line| **line == "***").count(), 4);
    assert!(ran.has_line("job succeeded"));
    assert_eq!(ran.code, Some(0));
    Ok(())
}
