//! Private gate coverage for data-only task execution resolution.

use std::error::Error;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn task_execution_resolver_requires_runner_gate_and_keeps_stdout_data_only()
-> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("gate-task-execution")?;
    let workspace = temp.join("workspace");
    let runner_temp = temp.join("runner-temp");
    std::fs::create_dir_all(&workspace)?;
    std::fs::create_dir_all(&runner_temp)?;

    let bare = spawn_isolated(&[], &[], &workspace)?;
    assert_eq!(code(&bare), 2);
    let no_run_id = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "resolve-task-execution-v1"),
            ("RUNNER_TEMP", runner_temp.to_str().unwrap_or("/")),
        ],
        &workspace,
    )?;
    assert_eq!(code(&no_run_id), 2);
    assert_eq!(bare.stdout, no_run_id.stdout);
    assert_eq!(bare.stderr, no_run_id.stderr);

    let execution_digest = format!("b3-{}", "a".repeat(64));
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "resolve-task-execution-v1"),
            ("RUNNER_TEMP", runner_temp.to_str().unwrap_or("/")),
            (
                "VELNOR_RUNTIME_RUNNER_TEMP",
                runner_temp.to_str().unwrap_or("/"),
            ),
            ("GITHUB_WORKSPACE", workspace.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "12345"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_TASK_EXECUTION_DIGEST", execution_digest.as_str()),
            ("VELNOR_GENERATOR_VERSION", "0.1.6"),
        ],
        &workspace,
    )?;
    assert_eq!(code(&output), 1);
    assert!(
        output.stdout.is_empty(),
        "failure must emit no partial frame"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing_task_execution_manifest"),
        "{stderr}"
    );
    assert!(
        !stderr.contains(&execution_digest),
        "digest is not diagnostic output"
    );

    let mismatch = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "resolve-task-execution-v1"),
            ("RUNNER_TEMP", runner_temp.to_str().unwrap_or("/")),
            (
                "VELNOR_RUNTIME_RUNNER_TEMP",
                workspace.to_str().unwrap_or("/"),
            ),
            ("GITHUB_WORKSPACE", workspace.to_str().unwrap_or("/")),
            ("GITHUB_RUN_ID", "12345"),
        ],
        &workspace,
    )?;
    assert_eq!(code(&mismatch), 1);
    assert!(
        mismatch.stdout.is_empty(),
        "mismatch emits no partial frame"
    );
    let mismatch_stderr = String::from_utf8_lossy(&mismatch.stderr);
    assert!(
        mismatch_stderr.contains("runner_temp_binding_mismatch"),
        "unexpected runner-temp mismatch diagnostic: {mismatch_stderr}"
    );
    assert!(
        !mismatch_stderr.contains(runner_temp.to_str().unwrap_or("/")),
        "runner path must not appear in diagnostics"
    );

    cleanup(&temp);
    Ok(())
}
