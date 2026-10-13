//! Env-gate matrix: the private entrypoint needs op plus request file,
//! and every public surface stays byte-identical with the env set.

use std::error::Error;
use std::path::{Path, PathBuf};

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

/// Stage `<dir>/<name>` with `body`, creating parents.
pub(super) fn stage_request(dir: &Path, name: &str, body: &str) -> Result<PathBuf, Box<dyn Error>> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Assert two outputs are byte-identical in code, stdout, and stderr.
pub(super) fn assert_identical(left: &std::process::Output, right: &std::process::Output) {
    assert_eq!(code(left), code(right));
    assert_eq!(left.stdout, right.stdout);
    assert_eq!(left.stderr, right.stderr);
}

/// Assert the JSON protocol ran: silent stdout, sibling response, no mode leak.
fn assert_protocol(output: &std::process::Output, dir: &Path, sibling: &str) {
    let status = code(output);
    assert!(status == 0 || status == 1, "internal exit was {status}");
    assert!(output.stdout.is_empty(), "internal stdout must stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!stderr.contains("plan-v1"));
    assert!(!stderr.contains("merge-v1"));
    assert!(!stderr.contains("write-request-v1"));
    assert!(!stderr.contains("VELNOR_INTERNAL"));
    if status == 0 {
        assert!(dir.join(sibling).is_file());
    }
}

/// The named regression: bare or private-looking argv without env is usage.
#[test]
fn internal_op_requires_env_gate_and_keeps_public_tree() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    assert_eq!(code(&bare), 2);
    let hidden = spawn_isolated(&["__internal"], &[], &tmp)?;
    assert_eq!(code(&hidden), 2);
    let hidden_plan = spawn_isolated(&["__internal-plan"], &[], &tmp)?;
    assert_eq!(code(&hidden_plan), 2);
    let help = spawn_isolated(&["--help"], &[], &tmp)?;
    assert_eq!(code(&help), 0);
    let help_text = String::from_utf8_lossy(&help.stdout).into_owned();
    assert!(!help_text.contains("__"));
    assert!(!help_text.contains("plan-v1"));
    assert!(!help_text.contains("merge-v1"));
    assert!(!help_text.contains("write-request-v1"));
    cleanup(&tmp);
    Ok(())
}

#[test]
fn plan_and_merge_without_request_file_match_bare() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-nofile")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    assert_eq!(code(&bare), 2);
    for op in ["plan-v1", "merge-v1"] {
        let missing = tmp.join(format!("{op}-request.json"));
        let gated = spawn_isolated(
            &[],
            &[
                ("VELNOR_INTERNAL_OP", op),
                ("VELNOR_REQUEST_FILE", missing.to_str().unwrap_or("/")),
            ],
            &tmp,
        )?;
        assert_eq!(code(&gated), 2);
        assert_identical(&bare, &gated);
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn write_request_without_github_env_matches_bare() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-wr-env")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    let missing = tmp.join("plan-v1-request.json");
    let gated = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "write-request-v1"),
            ("VELNOR_REQUEST_FILE", missing.to_str().unwrap_or("/")),
        ],
        &tmp,
    )?;
    assert_identical(&bare, &gated);
    assert!(!missing.exists());
    cleanup(&tmp);
    Ok(())
}

#[test]
fn write_request_with_existing_file_matches_bare() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-wr-exists")?;
    let dir = tmp.join("request");
    let staged = stage_request(&dir, "plan-v1-request.json", "{}")?;
    let payload = stage_request(&dir, "event.json", "{}")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    let gated = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "write-request-v1"),
            ("VELNOR_REQUEST_FILE", staged.to_str().unwrap_or("/")),
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_EVENT_PATH", payload.to_str().unwrap_or("/")),
        ],
        &tmp,
    )?;
    assert_eq!(code(&gated), 2);
    assert_identical(&bare, &gated);
    assert_eq!(std::fs::read_to_string(&staged)?, "{}");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn unknown_op_matches_bare() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-op")?;
    let dir = tmp.join("request");
    let staged = stage_request(&dir, "plan-v1-request.json", "{}")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    let gated = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "bogus-v9"),
            ("VELNOR_REQUEST_FILE", staged.to_str().unwrap_or("/")),
        ],
        &tmp,
    )?;
    assert_eq!(code(&gated), 2);
    assert_identical(&bare, &gated);
    assert!(!dir.join("plan-v1-response.json").exists());
    cleanup(&tmp);
    Ok(())
}

#[test]
fn missing_gate_pieces_match_bare() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-missing")?;
    let dir = tmp.join("request");
    let staged = stage_request(&dir, "plan-v1-request.json", "{}")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    let file = staged.to_str().unwrap_or("/");
    for vars in [
        vec![("VELNOR_INTERNAL_OP", "plan-v1")],
        vec![("VELNOR_REQUEST_FILE", file)],
        vec![("VELNOR_INTERNAL_OP", ""), ("VELNOR_REQUEST_FILE", file)],
        vec![
            ("VELNOR_INTERNAL_OP", "plan-v1"),
            ("VELNOR_REQUEST_FILE", ""),
        ],
    ] {
        let gated = spawn_isolated(&[], &vars, &tmp)?;
        assert_identical(&bare, &gated);
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn help_and_version_identical_with_env() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-help")?;
    let dir = tmp.join("request");
    let staged = stage_request(&dir, "plan-v1-request.json", "{}")?;
    let file = staged.to_str().unwrap_or("/").to_owned();
    for flag in ["--help", "--version"] {
        let plain = spawn_isolated(&[flag], &[], &tmp)?;
        assert_eq!(code(&plain), 0);
        for op in ["write-request-v1", "plan-v1", "merge-v1", "bogus-v9"] {
            let gated = spawn_isolated(
                &[flag],
                &[("VELNOR_INTERNAL_OP", op), ("VELNOR_REQUEST_FILE", &file)],
                &tmp,
            )?;
            assert_identical(&plain, &gated);
        }
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn public_commands_ignore_internal_env() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-public")?;
    let dir = tmp.join("request");
    let staged = stage_request(&dir, "plan-v1-request.json", "{}")?;
    let file = staged.to_str().unwrap_or("/").to_owned();
    for args in [
        vec!["__internal"],
        vec!["plan", "--format", "json"],
        vec!["init", "extra"],
    ] {
        let plain = spawn_isolated(&args, &[], &tmp)?;
        assert_eq!(code(&plain), 2);
        let gated = spawn_isolated(
            &args,
            &[
                ("VELNOR_INTERNAL_OP", "plan-v1"),
                ("VELNOR_REQUEST_FILE", &file),
            ],
            &tmp,
        )?;
        assert_identical(&plain, &gated);
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn unreadable_request_exits_one_without_response() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    let tmp = fresh_tempdir("gate-unreadable")?;
    let dir = tmp.join("request");
    let staged = stage_request(&dir, "plan-v1-request.json", "{}")?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o000))?;
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "plan-v1"),
            ("VELNOR_REQUEST_FILE", staged.to_str().unwrap_or("/")),
        ],
        &tmp,
    )?;
    assert_eq!(code(&output), 1);
    assert!(output.stdout.is_empty());
    assert!(!dir.join("plan-v1-response.json").exists());
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600))?;
    cleanup(&tmp);
    Ok(())
}

#[test]
fn staged_plan_request_runs_json_protocol() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-plan")?;
    let dir = tmp.join("request");
    let staged = stage_request(&dir, "plan-v1-request.json", "{}")?;
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "plan-v1"),
            ("VELNOR_REQUEST_FILE", staged.to_str().unwrap_or("/")),
        ],
        &tmp,
    )?;
    assert_protocol(&output, &dir, "plan-v1-response.json");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn staged_merge_request_runs_json_protocol() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-merge")?;
    let dir = tmp.join("request");
    let staged = stage_request(&dir, "merge-v1-request.json", "{}")?;
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "merge-v1"),
            ("VELNOR_REQUEST_FILE", staged.to_str().unwrap_or("/")),
        ],
        &tmp,
    )?;
    assert_protocol(&output, &dir, "merge-v1-response.json");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn report_op_needs_runner_temp_and_run_id() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-report")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    for vars in [
        vec![("VELNOR_INTERNAL_OP", "write-task-report-v1")],
        vec![
            ("VELNOR_INTERNAL_OP", "write-task-report-v1"),
            ("GITHUB_RUN_ID", "7"),
        ],
        vec![
            ("VELNOR_INTERNAL_OP", "write-task-report-v1"),
            ("RUNNER_TEMP", tmp.to_str().unwrap_or("/")),
        ],
    ] {
        let gated = spawn_isolated(&[], &vars, &tmp)?;
        assert_eq!(code(&gated), 2);
        assert_identical(&bare, &gated);
    }
    cleanup(&tmp);
    Ok(())
}

// NOTE: `write-preseed-manifest-v1` gate coverage lives in
// `impl_cli_gate_preseed.rs` (alint `rust-max-lines` split).

#[test]
fn report_op_without_plan_fails_internal_silently() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-report-run")?;
    let runner = tmp.to_str().unwrap_or("/").to_owned();
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "write-task-report-v1"),
            ("RUNNER_TEMP", runner.as_str()),
            ("GITHUB_RUN_ID", "7"),
            ("GITHUB_RUN_ATTEMPT", "2"),
            ("VELNOR_TASK_ID", "stack/rust/demo/clippy/default"),
            ("VELNOR_EXIT_CODE", "0"),
        ],
        &tmp,
    )?;
    assert_eq!(code(&output), 1);
    assert!(output.stdout.is_empty(), "internal stdout must stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(stderr.contains("internal request failed"), "{stderr}");
    assert!(!stderr.contains("write-task-report-v1"), "{stderr}");
    assert!(!stderr.contains("VELNOR_INTERNAL"), "{stderr}");
    cleanup(&tmp);
    Ok(())
}
