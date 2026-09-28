//! Env-gate matrix: the private entrypoint needs op plus request file,
//! and every public surface stays byte-identical with the env set.

use std::error::Error;
use std::path::{Path, PathBuf};

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn};

/// Run one child with the internal env set.
fn with_internal_env(
    args: &[&str],
    op: Option<&str>,
    runner_temp: Option<&Path>,
    run_key: Option<&str>,
    cwd: &Path,
) -> Result<std::process::Output, Box<dyn Error>> {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command.args(args).current_dir(cwd);
    if let Some(op) = op {
        command.env("VELNOR_INTERNAL_OP", op);
    }
    if let Some(temp) = runner_temp {
        command.env("RUNNER_TEMP", temp);
    }
    if let Some(key) = run_key {
        command.env("VELNOR_RUN_KEY", key);
    }
    Ok(command.output()?)
}

/// Stage `$RUNNER_TEMP/velnor/<key>/request.json` with `body`.
fn stage_request(temp: &Path, key: &str, body: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = temp.join("velnor").join(key);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("request.json"), body)?;
    Ok(dir)
}

/// Assert two outputs are byte-identical in code, stdout, and stderr.
fn assert_identical(left: &std::process::Output, right: &std::process::Output) {
    assert_eq!(code(left), code(right));
    assert_eq!(left.stdout, right.stdout);
    assert_eq!(left.stderr, right.stderr);
}

/// The named regression: bare or private-looking argv without env is usage.
#[test]
fn internal_op_requires_env_gate_and_keeps_public_tree() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate")?;
    let bare = spawn(&[], &[], &tmp)?;
    assert_eq!(code(&bare), 2);
    let hidden = spawn(&["__internal"], &[], &tmp)?;
    assert_eq!(code(&hidden), 2);
    let hidden_plan = spawn(&["__internal-plan"], &[], &tmp)?;
    assert_eq!(code(&hidden_plan), 2);
    let help = spawn(&["--help"], &[], &tmp)?;
    assert_eq!(code(&help), 0);
    let help_text = String::from_utf8_lossy(&help.stdout).into_owned();
    assert!(!help_text.contains("__"));
    assert!(!help_text.contains("plan-v1"));
    assert!(!help_text.contains("merge-v1"));
    cleanup(&tmp);
    Ok(())
}

#[test]
fn env_without_request_file_matches_bare() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-nofile")?;
    let bare = spawn(&[], &[], &tmp)?;
    assert_eq!(code(&bare), 2);
    for op in ["plan-v1", "merge-v1"] {
        let gated = with_internal_env(&[], Some(op), Some(&tmp), Some("r1"), &tmp)?;
        assert_eq!(code(&gated), 2);
        assert_identical(&bare, &gated);
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn unknown_op_matches_bare() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-op")?;
    let dir = stage_request(&tmp, "r1", "{}")?;
    let bare = spawn(&[], &[], &tmp)?;
    let gated = with_internal_env(&[], Some("bogus-v9"), Some(&tmp), Some("r1"), &tmp)?;
    assert_eq!(code(&gated), 2);
    assert_identical(&bare, &gated);
    assert!(!dir.join("response.json").exists());
    cleanup(&tmp);
    Ok(())
}

#[test]
fn missing_gate_pieces_match_bare() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-missing")?;
    let _ = stage_request(&tmp, "r1", "{}")?;
    let bare = spawn(&[], &[], &tmp)?;
    let no_temp = with_internal_env(&[], Some("plan-v1"), None, Some("r1"), &tmp)?;
    assert_identical(&bare, &no_temp);
    let no_key = with_internal_env(&[], Some("plan-v1"), Some(&tmp), None, &tmp)?;
    assert_identical(&bare, &no_key);
    let no_op = with_internal_env(&[], None, Some(&tmp), Some("r1"), &tmp)?;
    assert_identical(&bare, &no_op);
    let empty_op = with_internal_env(&[], Some(""), Some(&tmp), Some("r1"), &tmp)?;
    assert_identical(&bare, &empty_op);
    let empty_key = with_internal_env(&[], Some("plan-v1"), Some(&tmp), Some(""), &tmp)?;
    assert_identical(&bare, &empty_key);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn help_and_version_identical_with_env() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-help")?;
    for flag in ["--help", "--version"] {
        let plain = spawn(&[flag], &[], &tmp)?;
        assert_eq!(code(&plain), 0);
        for op in ["plan-v1", "merge-v1", "bogus-v9"] {
            let gated = with_internal_env(&[flag], Some(op), Some(&tmp), Some("r1"), &tmp)?;
            assert_identical(&plain, &gated);
        }
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn public_commands_ignore_internal_env() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-public")?;
    for args in [
        vec!["__internal"],
        vec!["plan", "--format", "json"],
        vec!["init", "extra"],
    ] {
        let plain = spawn(&args, &[], &tmp)?;
        assert_eq!(code(&plain), 2);
        let gated = with_internal_env(&args, Some("plan-v1"), Some(&tmp), Some("r1"), &tmp)?;
        assert_identical(&plain, &gated);
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn traversal_run_key_is_refused() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-traversal")?;
    let outer = tmp.join("outer");
    let target = outer.join("velnor").join("victim");
    std::fs::create_dir_all(&target)?;
    std::fs::write(target.join("request.json"), "{}")?;
    let bare = spawn(&[], &[], &tmp)?;
    for key in ["../outer/velnor/victim", "..", ".", "a/b", "a\\b"] {
        let gated = with_internal_env(&[], Some("plan-v1"), Some(&outer), Some(key), &tmp)?;
        assert_eq!(code(&gated), 2, "key {key} must be refused");
        assert_identical(&bare, &gated);
    }
    assert!(!target.join("response.json").exists());
    cleanup(&tmp);
    Ok(())
}

/// Assert the JSON protocol ran: silent stdout, response file, no mode leak.
fn assert_protocol(output: &std::process::Output, dir: &Path) {
    let status = code(output);
    assert!(status == 0 || status == 1, "internal exit was {status}");
    assert!(output.stdout.is_empty(), "internal stdout must stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!stderr.contains("plan-v1"));
    assert!(!stderr.contains("merge-v1"));
    assert!(!stderr.contains("VELNOR_INTERNAL"));
    if status == 0 {
        assert!(dir.join("response.json").is_file());
    }
}

#[test]
fn unreadable_request_exits_one_without_response() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    let tmp = fresh_tempdir("gate-unreadable")?;
    let dir = stage_request(&tmp, "r1", "{}")?;
    std::fs::set_permissions(
        dir.join("request.json"),
        std::fs::Permissions::from_mode(0o000),
    )?;
    let output = with_internal_env(&[], Some("plan-v1"), Some(&tmp), Some("r1"), &tmp)?;
    assert_eq!(code(&output), 1);
    assert!(output.stdout.is_empty());
    assert!(!dir.join("response.json").exists());
    std::fs::set_permissions(
        dir.join("request.json"),
        std::fs::Permissions::from_mode(0o600),
    )?;
    cleanup(&tmp);
    Ok(())
}

#[test]
fn staged_plan_request_runs_json_protocol() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-plan")?;
    let dir = stage_request(&tmp, "r1", "{}")?;
    let output = with_internal_env(&[], Some("plan-v1"), Some(&tmp), Some("r1"), &tmp)?;
    assert_protocol(&output, &dir);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn staged_merge_request_runs_json_protocol() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-merge")?;
    let dir = stage_request(&tmp, "r1", "{}")?;
    let output = with_internal_env(&[], Some("merge-v1"), Some(&tmp), Some("r1"), &tmp)?;
    assert_protocol(&output, &dir);
    cleanup(&tmp);
    Ok(())
}
