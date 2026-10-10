//! Per-policy spawn-env contract: credential allowlists, proxy
//! passthrough, and explicit repo-task env over fake parent snapshots.
//!
//! Pure `spawn_env` cases run everywhere; the live ambient-proxy case
//! needs a POSIX shell and is unix-gated. No `set_var`: the pure cases
//! use fake parents, the live case only reads ambient proxy keys.
use std::ffi::OsString;
use velnor_actions_mise::command::{
    CREDENTIAL_ALLOWLIST_BASELINE, CREDENTIAL_ALLOWLIST_BOOTSTRAP, CREDENTIAL_ENV_KEYS, EnvPolicy,
    IsolatedCommand, PROXY_ENV_KEYS,
};
use velnor_actions_mise::{GitRequest, PinnedTool, PinnedToolExec, ToolCatalog};

fn pair(key: &str, value: &str) -> (OsString, OsString) {
    (OsString::from(key), OsString::from(value))
}

fn parent_snapshot() -> Vec<(OsString, OsString)> {
    let mut parent = vec![pair("PATH", "/usr/bin:/bin"), pair("VELNOR_OTHER", "keep")];
    for key in CREDENTIAL_ENV_KEYS {
        parent.push(pair(key, "__SENTINEL__"));
    }
    for key in PROXY_ENV_KEYS {
        parent.push(pair(key, "http://proxy.invalid:8080"));
    }
    parent
}

fn has(env: &[(OsString, OsString)], key: &str) -> bool {
    env.iter().any(|(name, _)| name == key)
}

fn hostile_tofu_parent() -> Vec<(OsString, OsString)> {
    let mut parent = parent_snapshot();
    for (key, value) in [
        ("TF_VAR_secret", "hunter2"),
        ("TF_CLI_ARGS", "-auto-approve"),
        ("TF_CLI_ARGS_plan", "-destroy"),
        ("TF_TOKEN_app", "hunter2"),
        ("TF_DATA_DIR", "/evil/data"),
        ("TF_CLI_CONFIG_FILE", "/evil/cli.hcl"),
        ("TF_PLUGIN_CACHE_DIR", "/evil/cache"),
        ("TF_WORKSPACE", "evil"),
        ("TF_LOG", "DEBUG"),
        ("TF_LOG_PATH", "/evil/tofu.log"),
        ("TF_REGISTRY_CLIENT_TIMEOUT", "0"),
        ("TOFU_FUTURE_KEY", "evil"),
        ("CHECKPOINT_DISABLE", "0"),
        ("TF_IN_AUTOMATION", "0"),
        ("TF_INPUT", "1"),
    ] {
        parent.push(pair(key, value));
    }
    parent
}

fn bootstrap() -> Result<IsolatedCommand, String> {
    IsolatedCommand::mise_install(&["rust@1.98.1".to_owned()]).map_err(|err| err.to_string())
}

fn baseline() -> Result<IsolatedCommand, String> {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        std::ffi::OsStr::new("gh"),
        vec![OsString::from("--version")],
    )
    .map_err(|err| err.to_string())?;
    exec.command(&ToolCatalog::pinned())
        .map_err(|err| err.to_string())
}

fn verify() -> Result<IsolatedCommand, String> {
    IsolatedCommand::mise_exec(
        &["rust@1.98.1".to_owned()],
        &[OsString::from("cargo"), OsString::from("--version")],
    )
    .map_err(|err| err.to_string())
}

fn discovery() -> IsolatedCommand {
    GitRequest::rev_parse(vec![OsString::from("--show-toplevel")]).command()
}

fn repo_task() -> Result<IsolatedCommand, String> {
    let declared = vec![pair("VELNOR_P07_DECLARED", "present")];
    IsolatedCommand::repo_task("sh", Vec::new(), &declared).map_err(|err| err.to_string())
}

/// Endpoint selectors never survive any policy: `GH_HOST` would
/// reroute `gh` (and any kept token) to an attacker host, and
/// `GH_CONFIG_DIR` would load attacker-controlled auth.
#[test]
fn endpoint_selectors_strip_for_every_policy() -> Result<(), String> {
    let mut parent = parent_snapshot();
    parent.push(pair("GH_HOST", "evil.example"));
    parent.push(pair("GH_CONFIG_DIR", "/tmp/evil"));
    parent.push(pair("GH_ENTERPRISE_TOKEN", "__SENTINEL__"));
    let cases: Vec<(&str, Vec<(OsString, OsString)>)> = vec![
        ("bootstrap", bootstrap()?.spawn_env(&parent)),
        ("baseline", baseline()?.spawn_env(&parent)),
        ("verify", verify()?.spawn_env(&parent)),
        ("discovery", discovery().spawn_env(&parent)),
        ("repo_task", repo_task()?.spawn_env(&parent)),
    ];
    for (label, env) in &cases {
        for key in ["GH_HOST", "GH_CONFIG_DIR", "GH_ENTERPRISE_TOKEN"] {
            assert!(!has(env, key), "{label}: {key} must strip: {env:?}");
        }
    }
    Ok(())
}

/// H1 hostile-ambient negatives: no tofu/checkpoint ambient value
/// survives any policy, including the automation pair (the constructor
/// sets its own values after the strip, never inheriting them).
#[test]
fn tofu_ambient_families_strip_for_every_policy() -> Result<(), String> {
    let parent = hostile_tofu_parent();
    let cases: Vec<(&str, Vec<(OsString, OsString)>)> = vec![
        ("bootstrap", bootstrap()?.spawn_env(&parent)),
        ("baseline", baseline()?.spawn_env(&parent)),
        ("verify", verify()?.spawn_env(&parent)),
        ("discovery", discovery().spawn_env(&parent)),
        ("repo_task", repo_task()?.spawn_env(&parent)),
    ];
    for (label, env) in &cases {
        for key in [
            "TF_VAR_secret",
            "TF_CLI_ARGS",
            "TF_CLI_ARGS_plan",
            "TF_TOKEN_app",
            "TF_DATA_DIR",
            "TF_CLI_CONFIG_FILE",
            "TF_PLUGIN_CACHE_DIR",
            "TF_WORKSPACE",
            "TF_LOG",
            "TF_LOG_PATH",
            "TF_REGISTRY_CLIENT_TIMEOUT",
            "TOFU_FUTURE_KEY",
            "CHECKPOINT_DISABLE",
            "TF_IN_AUTOMATION",
            "TF_INPUT",
        ] {
            assert!(!has(env, key), "{label}: ambient {key} must strip: {env:?}");
        }
        for (key, value) in env {
            assert_ne!(
                value.to_string_lossy(),
                "hunter2",
                "{label}: hostile value must not survive via {key:?}"
            );
        }
    }
    Ok(())
}

/// Strip-then-set ordering: the tofu constructor's baked pairs apply
/// after the ambient strip, so hostile parent values cannot shadow
/// Velnor's own isolation env.
#[test]
fn tofu_exec_baked_env_survives_hostile_parent() -> Result<(), String> {
    let command = IsolatedCommand::tofu_exec(
        &["opentofu@1.13.1".to_owned()],
        &[OsString::from("tofu"), OsString::from("version")],
        "/velnor/data",
        "/velnor/cli.hcl",
        "/velnor/cache",
    )
    .map_err(|err| err.to_string())?;
    let env = command.spawn_env(&hostile_tofu_parent());
    for (key, value) in [
        ("TF_IN_AUTOMATION", "1"),
        ("TF_INPUT", "0"),
        ("TF_DATA_DIR", "/velnor/data"),
        ("TF_CLI_CONFIG_FILE", "/velnor/cli.hcl"),
        ("TF_PLUGIN_CACHE_DIR", "/velnor/cache"),
    ] {
        let seen: Vec<OsString> = env
            .iter()
            .filter(|(name, _)| *name == key)
            .map(|(_, val)| val.clone())
            .collect();
        assert_eq!(
            seen,
            [OsString::from(value)],
            "{key} must carry exactly the baked value: {env:?}"
        );
    }
    Ok(())
}

#[test]
fn proxy_allowlist_is_exact() {
    assert_eq!(
        PROXY_ENV_KEYS,
        [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "NO_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "no_proxy",
            "all_proxy",
        ]
    );
}

#[test]
fn credential_allowlists_are_exact() {
    assert_eq!(
        CREDENTIAL_ALLOWLIST_BOOTSTRAP,
        ["MISE_GITHUB_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"]
    );
    assert_eq!(CREDENTIAL_ALLOWLIST_BASELINE, ["GITHUB_TOKEN", "GH_TOKEN"]);
    assert_eq!(EnvPolicy::Verify.allowed_credentials(), []);
    assert_eq!(EnvPolicy::Discovery.allowed_credentials(), []);
    assert_eq!(EnvPolicy::RepoTask.allowed_credentials(), []);
    assert!(EnvPolicy::Bootstrap.inherits_parent());
    assert!(EnvPolicy::Baseline.inherits_parent());
    assert!(!EnvPolicy::RepoTask.inherits_parent());
}

#[test]
fn bootstrap_keeps_only_download_credentials() -> Result<(), String> {
    let env = bootstrap()?.spawn_env(&parent_snapshot());
    for key in CREDENTIAL_ALLOWLIST_BOOTSTRAP {
        assert!(has(&env, key), "{key} must survive bootstrap");
    }
    for key in [
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "CARGO_REGISTRY_TOKEN",
    ] {
        assert!(!has(&env, key), "{key} must be stripped: {env:?}");
    }
    assert!(has(&env, "PATH"), "non-credential parent survives");
    for key in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
    ] {
        assert!(has(&env, key), "full isolation applies: {env:?}");
    }
    Ok(())
}

#[test]
fn baseline_keeps_only_ci_identity() -> Result<(), String> {
    let env = baseline()?.spawn_env(&parent_snapshot());
    for key in CREDENTIAL_ALLOWLIST_BASELINE {
        assert!(has(&env, key), "{key} must survive baseline");
    }
    for key in [
        "MISE_GITHUB_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "CARGO_REGISTRY_TOKEN",
    ] {
        assert!(!has(&env, key), "{key} must be stripped: {env:?}");
    }
    assert!(has(&env, "PATH"), "non-credential parent survives");
    Ok(())
}

#[test]
fn verify_and_discovery_strip_every_credential() -> Result<(), String> {
    for command in [verify()?, discovery()] {
        let env = command.spawn_env(&parent_snapshot());
        for key in CREDENTIAL_ENV_KEYS {
            assert!(!has(&env, key), "{key} must be stripped: {env:?}");
        }
        assert!(has(&env, "PATH"), "non-credential parent survives");
        assert!(has(&env, "HTTP_PROXY"), "proxy survives inheritance");
    }
    Ok(())
}

#[test]
fn repo_task_env_is_proxy_plus_explicit_only() -> Result<(), String> {
    let env = repo_task()?.spawn_env(&parent_snapshot());
    for key in CREDENTIAL_ENV_KEYS {
        assert!(!has(&env, key), "{key} must be stripped: {env:?}");
    }
    assert!(!has(&env, "PATH"), "PATH needs declaration: {env:?}");
    assert!(!has(&env, "VELNOR_OTHER"), "ambient shorts drop: {env:?}");
    for key in PROXY_ENV_KEYS {
        assert!(has(&env, key), "proxy {key} passes through: {env:?}");
    }
    assert!(
        env.iter()
            .any(|(key, value)| key == "VELNOR_P07_DECLARED" && value == "present"),
        "declared inputs arrive: {env:?}"
    );
    assert!(has(&env, "MISE_NO_CONFIG"), "overlay applies: {env:?}");
    Ok(())
}

#[test]
fn gh_exec_selects_baseline_policy() -> Result<(), String> {
    let gh = format!("{:?}", baseline()?);
    assert!(
        gh.contains("Baseline"),
        "gh lookup must carry Baseline: {gh}"
    );
    let cargo = format!("{:?}", verify()?);
    assert!(cargo.contains("Verify"), "exec stays Verify: {cargo}");
    Ok(())
}

#[test]
#[cfg(unix)]
fn live_repo_task_keeps_ambient_proxy_only() -> Result<(), String> {
    let declared = vec![pair("VELNOR_P07_DECLARED", "present")];
    let task = IsolatedCommand::repo_task("/usr/bin/env", Vec::new(), &declared)
        .map_err(|err| err.to_string())?;
    let output = task.run().map_err(|err| err.to_string())?;
    assert!(output.success, "env must run: {output:?}");
    let text = output.stdout_text("env").map_err(|err| err.to_string())?;
    for key in CREDENTIAL_ENV_KEYS {
        assert!(
            !text
                .lines()
                .any(|line| line.starts_with(&format!("{key}="))),
            "credential {key} must be absent:\n{text}"
        );
        if let Ok(value) = std::env::var(key) {
            // An empty ambient value carries no secret; `contains("")`
            // is vacuously true, so only non-empty values are probed.
            if !value.is_empty() {
                assert!(
                    !text.contains(&value),
                    "ambient {key} value must be absent:\n{text}"
                );
            }
        }
    }
    for key in PROXY_ENV_KEYS {
        let Ok(value) = std::env::var(key) else {
            continue;
        };
        assert!(
            text.lines().any(|line| line == format!("{key}={value}")),
            "ambient proxy {key} must pass through:\n{text}"
        );
    }
    assert!(
        text.lines()
            .any(|line| line == "VELNOR_P07_DECLARED=present"),
        "declared inputs arrive:\n{text}"
    );
    Ok(())
}

#[path = "impl_mise_mbx_env_policy.rs"]
mod mbx_env_policy;
