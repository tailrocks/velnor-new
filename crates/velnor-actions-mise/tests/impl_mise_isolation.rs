//! P07 isolation cases: typed policies, reserved keys, cleared repo-task
//! env, and bounded runs with real exit/signal preserved.
//!
//! Live-spawn cases need a POSIX shell; the whole module is unix-gated.
#![cfg(unix)]

use std::ffi::OsString;
use std::time::Duration;
use velnor_actions_mise::command::{
    EnvPolicy, IsolatedCommand, OUTPUT_CAPTURE_LIMIT_BYTES, RUN_TIMEOUT_SECS, is_reserved_env_key,
};
use velnor_actions_mise::{GitRequest, MiseError};

fn specs() -> Vec<String> {
    vec!["rust@1.98.1".to_owned()]
}

fn payload() -> Vec<OsString> {
    vec![OsString::from("cargo"), OsString::from("--version")]
}

fn policy_of(command: &IsolatedCommand) -> Result<EnvPolicy, String> {
    let debug = format!("{command:?}");
    for policy in [
        EnvPolicy::Bootstrap,
        EnvPolicy::Verify,
        EnvPolicy::Discovery,
        EnvPolicy::RepoTask,
    ] {
        if debug.contains(&format!("{policy:?}")) {
            return Ok(policy);
        }
    }
    Err(format!("no policy in {debug:?}"))
}

#[test]
fn constructors_assign_typed_policies() -> Result<(), String> {
    let exec = IsolatedCommand::mise_exec(&specs(), &payload()).map_err(|err| err.to_string())?;
    assert_eq!(policy_of(&exec)?, EnvPolicy::Verify);
    let install = IsolatedCommand::mise_install(&specs()).map_err(|err| err.to_string())?;
    assert_eq!(policy_of(&install)?, EnvPolicy::Bootstrap);
    let git = GitRequest::rev_parse(vec![OsString::from("--show-toplevel")]).command();
    assert_eq!(policy_of(&git)?, EnvPolicy::Discovery);
    let task = IsolatedCommand::repo_task("sh", Vec::new(), &[]).map_err(|err| err.to_string())?;
    assert_eq!(policy_of(&task)?, EnvPolicy::RepoTask);
    Ok(())
}

#[test]
fn reserved_keys_cover_isolation_disable_and_credentials() {
    for key in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
        "MISE_AUTO_INSTALL",
        "MISE_EXEC_AUTO_INSTALL",
        "MISE_GITHUB_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
    ] {
        assert!(is_reserved_env_key(key), "{key} must be reserved");
    }
    for key in [
        "MISE_RUSTUP_HOME",
        "MISE_CARGO_HOME",
        "RUSTUP_TOOLCHAIN",
        "PATH",
        "VELNOR_TASK_RUN",
    ] {
        assert!(!is_reserved_env_key(key), "{key} must stay allowed");
    }
}

#[test]
fn reserved_override_rejected() -> Result<(), String> {
    let exec = IsolatedCommand::mise_exec(&specs(), &payload()).map_err(|err| err.to_string())?;
    let hostile: Vec<(OsString, OsString)> = [
        ("MISE_NO_CONFIG", "0"),
        ("MISE_AUTO_INSTALL", "true"),
        ("MISE_EXEC_AUTO_INSTALL", "true"),
        ("MISE_GITHUB_TOKEN", "sentinel"),
        ("GITHUB_TOKEN", "sentinel"),
    ]
    .iter()
    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
    .collect();
    let extended = exec.with_env(&hostile);
    let full = extended.full_env();
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
    ] {
        let hits: Vec<&OsString> = full
            .iter()
            .filter(|(found, _)| found == key)
            .map(|(_, seen)| seen)
            .collect();
        assert_eq!(
            hits.as_slice(),
            [value],
            "reserved {key} must keep its value"
        );
    }
    assert!(
        !full
            .iter()
            .any(|(key, _)| key == "MISE_GITHUB_TOKEN" || key == "GITHUB_TOKEN"),
        "credentials must never enter extras: {full:?}"
    );
    assert!(extended.disables_auto_install());
    Ok(())
}

#[test]
fn repo_task_rejects_reserved_declared_keys() {
    for key in [
        "MISE_NO_CONFIG",
        "MISE_AUTO_INSTALL",
        "MISE_GITHUB_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
    ] {
        let declared = vec![(OsString::from(key), OsString::from("x"))];
        assert!(
            matches!(
                IsolatedCommand::repo_task("sh", Vec::new(), &declared),
                Err(MiseError::InvalidStepInput { field, value })
                    if field == key && value == "reserved_env_key"
            ),
            "{key} must fail typed"
        );
    }
    let declared = vec![(OsString::from("VELNOR_TASK_RUN"), OsString::from("ok"))];
    assert!(IsolatedCommand::repo_task("sh", Vec::new(), &declared).is_ok());
}

#[test]
fn sentinel_credential_absent() -> Result<(), String> {
    // No `set_var`: edition 2024 marks it unsafe and this repo forbids
    // unsafe. Clearing is proven by ambient PATH instead: it is always
    // present in the test process, so its absence in the child proves
    // `env_clear`, and credentials additionally fail as declared inputs
    // (see `repo_task_rejects_reserved_declared_keys`), so neither path
    // can carry them. Run the suite with sentinel credentials exported
    // to also assert their values absent; no secret is copied here.
    assert!(
        std::env::var_os("PATH").is_some(),
        "test needs ambient PATH"
    );
    let declared = vec![(
        OsString::from("VELNOR_P07_DECLARED"),
        OsString::from("present"),
    )];
    let task = IsolatedCommand::repo_task("/usr/bin/env", Vec::new(), &declared)
        .map_err(|err| err.to_string())?;
    let output = task.run().map_err(|err| err.to_string())?;
    assert!(output.success, "env must run: {output:?}");
    let text = output.stdout_text("env").map_err(|err| err.to_string())?;
    for key in [
        "MISE_GITHUB_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
    ] {
        assert!(
            !text
                .lines()
                .any(|line| line.starts_with(&format!("{key}="))),
            "credential {key} must be absent:\n{text}"
        );
        if let Ok(value) = std::env::var(key) {
            assert!(
                !text.contains(&value),
                "ambient {key} value must be absent:\n{text}"
            );
        }
    }
    assert!(
        !text.lines().any(|line| line.starts_with("PATH=")),
        "ambient PATH must not leak without declaration:\n{text}"
    );
    for pair in [
        "MISE_NO_CONFIG=1",
        "MISE_NO_ENV=1",
        "MISE_NO_HOOKS=1",
        "MISE_LOCKFILE=0",
        "VELNOR_P07_DECLARED=present",
    ] {
        assert!(
            text.lines().any(|line| line == pair),
            "missing {pair}:\n{text}"
        );
    }
    let declared_path = vec![(OsString::from("PATH"), OsString::from("/declared/bin"))];
    let task = IsolatedCommand::repo_task("/usr/bin/env", Vec::new(), &declared_path)
        .map_err(|err| err.to_string())?;
    let output = task.run().map_err(|err| err.to_string())?;
    let text = output.stdout_text("env").map_err(|err| err.to_string())?;
    assert!(
        text.lines().any(|line| line == "PATH=/declared/bin"),
        "declared PATH must arrive exactly:\n{text}"
    );
    Ok(())
}

#[test]
fn default_bounds_match_named_consts() {
    assert_eq!(OUTPUT_CAPTURE_LIMIT_BYTES, 8 * 1024 * 1024);
    assert_eq!(RUN_TIMEOUT_SECS, 600);
}

#[test]
fn over_limit_stream_fails_closed() -> Result<(), String> {
    let task = IsolatedCommand::repo_task(
        "/bin/sh",
        vec![
            OsString::from("-c"),
            OsString::from("yes x | head -c 100000"),
        ],
        &[],
    )
    .map_err(|err| err.to_string())?;
    let err = task
        .run_bounded(1024, Duration::from_secs(60))
        .map_err(|err| err.to_string())
        .expect_err("100KB past a 1KB cap must fail");
    assert!(err.contains("stdout_limit_exceeded:1024"), "got {err}");
    let control = IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from("echo small")],
        &[],
    )
    .map_err(|err| err.to_string())?;
    let output = control
        .run_bounded(1024, Duration::from_secs(60))
        .map_err(|err| err.to_string())?;
    assert!(output.success);
    assert_eq!(
        output.stdout_text("sh").map_err(|err| err.to_string())?,
        "small\n"
    );
    Ok(())
}

#[test]
fn timeout_kills_and_reports_timeout() -> Result<(), String> {
    let task = IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from("sleep 5")],
        &[],
    )
    .map_err(|err| err.to_string())?;
    let start = std::time::Instant::now();
    let err = task
        .run_bounded(1024, Duration::from_secs(1))
        .map_err(|err| err.to_string())
        .expect_err("sleep past a 1s deadline must fail");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "child must be killed"
    );
    assert!(err.contains("timeout_after_secs:1"), "got {err}");
    Ok(())
}

#[test]
fn real_exit_and_signal_preserved() -> Result<(), String> {
    let task = IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from("exit 42")],
        &[],
    )
    .map_err(|err| err.to_string())?;
    let output = task
        .run_bounded(1024, Duration::from_secs(60))
        .map_err(|err| err.to_string())?;
    assert!(!output.success);
    assert_eq!(output.code, Some(42));
    assert_eq!(output.signal, None);
    assert!(matches!(
        output.require_success("sh"),
        Err(MiseError::NonZeroExit { code: Some(42), .. })
    ));
    let task = IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from("kill -9 $$")],
        &[],
    )
    .map_err(|err| err.to_string())?;
    let output = task
        .run_bounded(1024, Duration::from_secs(60))
        .map_err(|err| err.to_string())?;
    assert!(!output.success);
    assert_eq!(output.code, None);
    assert_eq!(output.signal, Some(9));
    Ok(())
}
