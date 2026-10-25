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
    // The Bun registry phase fails closed on an empty job token the same way.
    let bun = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "bun-registry-install-v1"),
            ("RUNNER_TEMP", &runner_temp),
            ("VELNOR_NPM_READ_TOKEN", ""),
            ("VELNOR_BUN_ROOT", "."),
            ("VELNOR_BUN_SCOPES", "@chainargos"),
            ("GITHUB_WORKSPACE", &runner_temp),
        ],
        &tmp,
    )?;
    assert_eq!(code(&bun), 1);
    assert!(
        String::from_utf8_lossy(&bun.stderr).contains("private_source_credential_unavailable"),
        "{}",
        String::from_utf8_lossy(&bun.stderr)
    );
    assert!(
        !tmp.join("velnor").exists(),
        "no credential directory is created"
    );
    cleanup(&tmp);
    Ok(())
}

#[test]
fn the_playwright_install_fails_closed_without_the_locks() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-playwright-install")?;
    let runner_temp = tmp.to_string_lossy().into_owned();
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "playwright-browsers-install-v1"),
            ("RUNNER_TEMP", &runner_temp),
            ("GITHUB_WORKSPACE", &runner_temp),
            ("VELNOR_PLAYWRIGHT_VERSION", "1.64.1"),
            ("VELNOR_PLAYWRIGHT_ROOT", "."),
            (
                "VELNOR_PLAYWRIGHT_PINS",
                "chromium-1248:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ),
        ],
        &tmp,
    )?;
    assert_eq!(code(&output), 1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("playwright_bun_lock_missing"), "{stderr}");
    assert!(
        !tmp.join("velnor/ms-playwright").exists(),
        "nothing is installed without the locks"
    );
    cleanup(&tmp);
    Ok(())
}

#[test]
fn private_operations_without_runner_temp_fall_through_to_usage() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-private-gate")?;
    for op in [
        "private-fetch-v1",
        "private-revoke-v1",
        "gradle-run-v1",
        "bun-registry-install-v1",
        "playwright-browsers-install-v1",
        "bun-assets-export-v1",
        "bun-assets-prepare-v1",
        "bun-assets-import-v1",
    ] {
        let gated = spawn_isolated(&[], &[("VELNOR_INTERNAL_OP", op)], &tmp)?;
        let bare = spawn_isolated(&[], &[], &tmp)?;
        assert_eq!(code(&gated), 2, "{op}");
        assert_eq!(gated.stderr, bare.stderr, "{op}: public behavior unchanged");
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn bun_assets_prepare_is_a_runner_temp_gated_internal_operation() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("smoke-bun-assets-prepare")?;
    let workspace = tmp.join("workspace");
    let runner = tmp.join("runner");
    std::fs::create_dir_all(workspace.join(".velnor/deps/stale"))?;
    std::fs::create_dir_all(runner.join("velnor"))?;
    std::fs::write(workspace.join(".velnor/deps/stale/file"), b"stale")?;
    let runner_text = runner.to_string_lossy().into_owned();
    let workspace_text = workspace.to_string_lossy().into_owned();
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "bun-assets-prepare-v1"),
            ("VELNOR_BUN_ASSET_ID", "bun-member"),
            (
                "VELNOR_BUN_ASSET_PINS",
                "ui-assets:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ),
            ("RUNNER_TEMP", &runner_text),
            ("GITHUB_WORKSPACE", &workspace_text),
        ],
        &tmp,
    )?;

    assert_eq!(
        code(&output),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"");
    assert!(std::fs::symlink_metadata(workspace.join(".velnor/deps")).is_err());
    assert!(runner.join("velnor/bun-assets/bun-member").is_dir());
    cleanup(&tmp);
    Ok(())
}
