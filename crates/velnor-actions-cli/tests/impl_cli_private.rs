//! Private-source command and operation gates (HD-7).

use std::error::Error;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, git_init, spawn, spawn_isolated};

#[test]
fn deps_lock_without_private_sources_exits_one_and_writes_nothing() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-deps-lock")?;
    git_init(&tmp)?;
    assert_eq!(code(&spawn(&["init"], &[], &tmp)?), 0);
    let output = spawn(&["deps", "lock"], &[], &tmp)?;
    assert_eq!(code(&output), 1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("private_sources") && stderr.contains("missing_private_sources"),
        "names the missing table: {stderr}"
    );
    assert!(!tmp.join(".velnor").join("deps.lock").exists());
    let help = spawn(&["deps", "--help"], &[], &tmp)?;
    assert_eq!(code(&help), 0);
    assert!(String::from_utf8_lossy(&help.stdout).contains("lock"));
    cleanup(&tmp);
    Ok(())
}

#[test]
fn private_operations_fail_closed_without_a_token_and_print_no_secret() -> Result<(), Box<dyn Error>>
{
    let tmp = fresh_tempdir("smoke-private-ops")?;
    let runner_temp = tmp.to_string_lossy().into_owned();
    for op in ["private-fetch-v1", "private-revoke-v1"] {
        // A fork pull request gets an empty token: the operation stops
        // before it reads any repository data or starts a child process.
        let output = spawn_isolated(
            &[],
            &[
                ("VELNOR_INTERNAL_OP", op),
                ("RUNNER_TEMP", &runner_temp),
                ("VELNOR_PRIVATE_FETCH_TOKEN", ""),
                ("VELNOR_PRIVATE_OWNER", "ChainArgos"),
                ("VELNOR_PRIVATE_REPOSITORIES", "processor-shared"),
                ("VELNOR_PRIVATE_SOURCES", "cargo"),
                ("GITHUB_WORKSPACE", &runner_temp),
            ],
            &tmp,
        )?;
        assert_eq!(code(&output), 1, "{op}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("private_source_credential_unavailable"),
            "{op}: {stderr}"
        );
    }
    assert!(
        !tmp.join("velnor").exists(),
        "no credential directory is created"
    );
    cleanup(&tmp);
    Ok(())
}

#[test]
fn private_operations_without_runner_temp_fall_through_to_usage() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-private-gate")?;
    for op in ["private-fetch-v1", "private-revoke-v1"] {
        let gated = spawn_isolated(&[], &[("VELNOR_INTERNAL_OP", op)], &tmp)?;
        let bare = spawn_isolated(&[], &[], &tmp)?;
        assert_eq!(code(&gated), 2, "{op}");
        assert_eq!(gated.stderr, bare.stderr, "{op}: public behavior unchanged");
    }
    cleanup(&tmp);
    Ok(())
}
