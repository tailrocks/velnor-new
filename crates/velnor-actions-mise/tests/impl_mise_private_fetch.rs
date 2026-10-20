//! HD-7 credential boundary: only the private-fetch policy keeps the two
//! private tokens, and no inheriting policy forwards a fetch-redirect key.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use velnor_actions_mise::command::{
    CREDENTIAL_ALLOWLIST_PRIVATE_FETCH, CREDENTIAL_ENV_KEYS, EnvPolicy, FETCH_REDIRECT_ENV_KEYS,
    IsolatedCommand, PROXY_ENV_KEYS, PrivateFetchCredential, is_fetch_redirect_key,
    is_reserved_env_key,
};

fn pair(key: &str, value: &str) -> (OsString, OsString) {
    (OsString::from(key), OsString::from(value))
}

fn has(env: &[(OsString, OsString)], key: &str) -> bool {
    env.iter().any(|(name, _)| name == key)
}

fn value<'a>(env: &'a [(OsString, OsString)], key: &str) -> Option<&'a OsString> {
    env.iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn hostile_parent() -> Vec<(OsString, OsString)> {
    let mut parent = vec![
        pair("PATH", "/usr/bin:/bin"),
        pair("VELNOR_OTHER_AMBIENT", "x"),
    ];
    for key in CREDENTIAL_ENV_KEYS {
        parent.push(pair(key, "ambient"));
    }
    for key in CREDENTIAL_ALLOWLIST_PRIVATE_FETCH {
        parent.push(pair(key, "ambient-private"));
    }
    for key in PROXY_ENV_KEYS {
        parent.push(pair(key, "http://proxy.invalid:8080"));
    }
    for key in [
        "GIT_ASKPASS",
        "SSH_ASKPASS",
        "GIT_CONFIG",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_CONFIG_PARAMETERS",
        "NPM_CONFIG_REGISTRY",
        "npm_config_registry",
        "BUN_CONFIG_REGISTRY",
    ] {
        parent.push(pair(key, "/evil"));
    }
    parent
}

fn fetch(credential: Option<(PrivateFetchCredential, &str)>) -> Result<IsolatedCommand, String> {
    IsolatedCommand::private_fetch(
        "git",
        vec![OsString::from("ls-remote")],
        PathBuf::from("/work"),
        Path::new("/runner/velnor/deps/gitconfig"),
        &[("MISE_CARGO_HOME".to_owned(), "/runner/cargo".to_owned())],
        credential,
    )
    .map_err(|err| err.to_string())
}

#[test]
fn the_allowlist_is_exactly_the_two_private_tokens() {
    assert_eq!(
        CREDENTIAL_ALLOWLIST_PRIVATE_FETCH,
        ["VELNOR_PRIVATE_FETCH_TOKEN", "VELNOR_NPM_READ_TOKEN"]
    );
    assert_eq!(
        EnvPolicy::PrivateFetch.allowed_credentials(),
        CREDENTIAL_ALLOWLIST_PRIVATE_FETCH
    );
    assert_eq!(
        PrivateFetchCredential::AppInstallation.env_key(),
        "VELNOR_PRIVATE_FETCH_TOKEN"
    );
    assert_eq!(
        PrivateFetchCredential::NpmRegistryRead.env_key(),
        "VELNOR_NPM_READ_TOKEN"
    );
    assert!(!EnvPolicy::PrivateFetch.inherits_parent());
    assert!(EnvPolicy::PrivateFetch.allows_proxy_passthrough());
}

#[test]
fn private_fetch_child_gets_one_credential_and_baked_git_isolation() -> Result<(), String> {
    let command = fetch(Some((
        PrivateFetchCredential::AppInstallation,
        "ghs_abc123",
    )))?;
    let env = command.spawn_env(&hostile_parent());
    assert_eq!(
        value(&env, "VELNOR_PRIVATE_FETCH_TOKEN"),
        Some(&OsString::from("ghs_abc123"))
    );
    for key in CREDENTIAL_ENV_KEYS {
        assert!(!has(&env, key), "{key} must not reach a fetch child");
    }
    assert!(
        !has(&env, "VELNOR_NPM_READ_TOKEN"),
        "ambient npm token must not leak: {env:?}"
    );
    assert_eq!(
        value(&env, "PATH"),
        std::env::var_os("PATH").as_ref(),
        "only the parent's own lookup path survives the clear: {env:?}"
    );
    assert!(!has(&env, "VELNOR_OTHER_AMBIENT"), "{env:?}");
    assert_eq!(
        value(&env, "MISE_CARGO_HOME"),
        Some(&OsString::from("/runner/cargo"))
    );
    for key in PROXY_ENV_KEYS {
        assert!(has(&env, key), "{key} proxy passthrough");
    }
    for forbidden in [
        "GIT_ASKPASS",
        "SSH_ASKPASS",
        "GIT_CONFIG",
        "BUN_CONFIG_REGISTRY",
    ] {
        assert!(!has(&env, forbidden), "{forbidden} leaked: {env:?}");
    }
    assert_eq!(
        value(&env, "GIT_CONFIG_GLOBAL"),
        Some(&OsString::from("/runner/velnor/deps/gitconfig"))
    );
    assert_eq!(
        value(&env, "GIT_CONFIG_NOSYSTEM"),
        Some(&OsString::from("1"))
    );
    assert!(!has(&env, "GIT_CONFIG_COUNT") && !has(&env, "GIT_CONFIG_KEY_0"));
    assert!(!has(&env, "NPM_CONFIG_REGISTRY") && !has(&env, "npm_config_registry"));
    Ok(())
}

#[test]
fn private_fetch_without_a_credential_carries_none() -> Result<(), String> {
    let env = fetch(None)?.spawn_env(&hostile_parent());
    for key in CREDENTIAL_ENV_KEYS
        .iter()
        .chain(CREDENTIAL_ALLOWLIST_PRIVATE_FETCH.iter())
    {
        assert!(!has(&env, key), "{key}");
    }
    Ok(())
}

#[test]
fn private_fetch_rejects_relative_config_and_unsafe_credentials() {
    let relative = IsolatedCommand::private_fetch(
        "git",
        Vec::new(),
        PathBuf::from("/work"),
        Path::new("relative/gitconfig"),
        &[],
        None,
    );
    assert!(relative.is_err());
    let foreign = IsolatedCommand::private_fetch(
        "git",
        Vec::new(),
        PathBuf::from("/work"),
        Path::new("/runner/gitconfig"),
        &[("GIT_ASKPASS".to_owned(), "/evil".to_owned())],
        None,
    );
    assert!(foreign.is_err(), "only owned toolchain keys are accepted");
    for bad in ["", "a b", "x\ny", "a;b", "$(x)", "\"q\""] {
        assert!(
            fetch(Some((PrivateFetchCredential::AppInstallation, bad))).is_err(),
            "{bad:?}"
        );
    }
}

#[test]
fn every_other_policy_strips_the_private_tokens() {
    let parent = vec![
        pair("VELNOR_PRIVATE_FETCH_TOKEN", "t"),
        pair("VELNOR_NPM_READ_TOKEN", "t"),
        pair("PATH", "/bin"),
    ];
    for policy in [
        EnvPolicy::Bootstrap,
        EnvPolicy::GhApi,
        EnvPolicy::Verify,
        EnvPolicy::Discovery,
        EnvPolicy::GitHubToken,
        EnvPolicy::ReleaseSigner,
        EnvPolicy::RepoTask,
        EnvPolicy::QualifiedCheck,
        EnvPolicy::QualifiedProbe,
        EnvPolicy::QualifiedAcquisition,
    ] {
        let env = policy.child_env(&parent, &[]);
        assert!(!has(&env, "VELNOR_PRIVATE_FETCH_TOKEN"), "{policy:?}");
        assert!(!has(&env, "VELNOR_NPM_READ_TOKEN"), "{policy:?}");
    }
}

#[test]
fn inheriting_policies_drop_every_fetch_redirect_key() {
    for policy in [
        EnvPolicy::Bootstrap,
        EnvPolicy::GhApi,
        EnvPolicy::Verify,
        EnvPolicy::Discovery,
    ] {
        let env = policy.child_env(&hostile_parent(), &[]);
        for (key, _) in &env {
            assert!(
                !is_fetch_redirect_key(&key.to_string_lossy()),
                "{policy:?} forwarded {key:?}"
            );
        }
        assert!(has(&env, "PATH"), "{policy:?} keeps ordinary parent keys");
    }
}

#[test]
fn callers_can_never_supply_a_private_token_as_a_declared_extra() {
    for key in CREDENTIAL_ALLOWLIST_PRIVATE_FETCH {
        assert!(is_reserved_env_key(key), "{key}");
        let task = IsolatedCommand::repo_task("true", Vec::new(), &[pair(key, "x")]);
        assert!(task.is_err(), "{key} must be rejected for repo tasks");
    }
}

#[test]
fn the_redirect_predicate_is_exact_and_pattern_complete() {
    for key in FETCH_REDIRECT_ENV_KEYS {
        assert!(is_fetch_redirect_key(key));
    }
    for key in [
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_KEY_17",
        "NPM_CONFIG_USERCONFIG",
        "npm_config_userconfig",
    ] {
        assert!(is_fetch_redirect_key(key), "{key}");
    }
    for key in [
        "GIT_TERMINAL_PROMPT",
        "GIT_DIR",
        "PATH",
        "NPM",
        "CARGO_HOME",
    ] {
        assert!(!is_fetch_redirect_key(key), "{key}");
    }
}
