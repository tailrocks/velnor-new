//! Toolchain environment and subcommand allowlist cases.
use std::ffi::OsString;
use velnor_actions_mise::command::{is_denied_credential_key, is_reserved_env_key};
use velnor_actions_mise::{
    ALLOWED_MISE_SUBCOMMANDS, CREDENTIAL_ENV_KEYS, GitRequest, IsolatedCommand,
    MISE_CARGO_HOME_ENV, MISE_RUSTUP_HOME_ENV, MiseError, NO_AUTO_INSTALL_ENV,
    RUSTUP_TOOLCHAIN_ENV, ToolCatalog, is_allowed_mise_subcommand, toolchain_env,
};

#[path = "impl_mise_env_tofu.rs"]
mod tofu;

#[test]
fn toolchain_env_names_are_exact() {
    assert_eq!(MISE_RUSTUP_HOME_ENV, "MISE_RUSTUP_HOME");
    assert_eq!(MISE_CARGO_HOME_ENV, "MISE_CARGO_HOME");
    assert_eq!(RUSTUP_TOOLCHAIN_ENV, "RUSTUP_TOOLCHAIN");
    assert_eq!(
        NO_AUTO_INSTALL_ENV,
        [
            ("MISE_AUTO_INSTALL", "false"),
            ("MISE_EXEC_AUTO_INSTALL", "false"),
        ]
    );
}

#[test]
fn rustup_toolchain_is_exact_catalog_pin() -> Result<(), String> {
    assert_eq!(ToolCatalog::pinned().rustup_toolchain(), "1.98.1");
    let catalog = ToolCatalog::new(
        "1.97.0", "1.19.0", "2.101.0", "1.7.12", "0.11.0", "1.30.1", "0.9.146", "1.13.0",
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(catalog.rustup_toolchain(), "1.97.0");
    Ok(())
}

#[test]
fn toolchain_env_has_exact_owned_roots_and_catalog_pin() {
    let expected = [
        ("MISE_DATA_DIR", "/velnor/mise"),
        ("RUSTUP_TOOLCHAIN", "1.98.1"),
        ("CARGO_HOME", "/velnor/cargo"),
        ("MISE_CARGO_HOME", "/velnor/cargo"),
        ("MISE_RUSTUP_HOME", "/velnor/rustup"),
        ("RUSTUP_HOME", "/velnor/rustup"),
    ]
    .into_iter()
    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
    .collect::<Vec<_>>();
    assert_eq!(
        toolchain_env("/velnor/rustup", "/velnor/cargo", "1.98.1"),
        expected,
    );
}

#[test]
fn exec_disables_implicit_install() -> Result<(), String> {
    let command = IsolatedCommand::mise_exec(
        &["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()],
        &[OsString::from("cargo"), OsString::from("--version")],
    )
    .map_err(|err| err.to_string())?;
    let full = command.full_env();
    for (key, value) in NO_AUTO_INSTALL_ENV {
        assert!(
            full.iter().any(|(found, val)| found == key && val == value),
            "missing disable pair {key}={value}: {full:?}"
        );
    }
    let overlay = IsolatedCommand::env_overlay();
    assert!(
        !overlay.iter().any(|(key, _)| key == "MISE_AUTO_INSTALL"),
        "shared overlay keeps the rustup install gate: {overlay:?}"
    );
    Ok(())
}

#[test]
fn auto_install_predicate_separates_exec_from_install() -> Result<(), String> {
    let exec = IsolatedCommand::mise_exec(
        &["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()],
        &[OsString::from("cargo"), OsString::from("--version")],
    )
    .map_err(|err| err.to_string())?;
    assert!(exec.disables_auto_install());
    let install = IsolatedCommand::mise_install(&[
        "rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned(),
    ])
    .map_err(|err| err.to_string())?;
    assert!(!install.disables_auto_install());
    let extended = exec
        .with_env(&toolchain_env("/velnor/rustup", "/velnor/cargo", "1.98.1"))
        .map_err(|err| err.to_string())?;
    assert!(extended.disables_auto_install());
    Ok(())
}

#[test]
fn install_and_direct_keep_install_enabled() -> Result<(), String> {
    let install = IsolatedCommand::mise_install(&[
        "rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned(),
    ])
    .map_err(|err| err.to_string())?;
    assert!(
        !install
            .full_env()
            .iter()
            .any(|(key, _)| key == "MISE_EXEC_AUTO_INSTALL"),
        "bootstrap install must install"
    );
    let git = GitRequest::rev_parse(vec![OsString::from("--show-toplevel")]).command();
    assert!(
        !git.full_env()
            .iter()
            .any(|(key, _)| key == "MISE_EXEC_AUTO_INSTALL"),
        "tool-free git needs no install gate"
    );
    Ok(())
}

#[test]
fn with_env_appends_toolchain_pairs() -> Result<(), String> {
    let command = IsolatedCommand::mise_exec(
        &["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()],
        &[OsString::from("cargo"), OsString::from("--version")],
    )
    .map_err(|err| err.to_string())?;
    let extra = toolchain_env("/velnor/rustup", "/velnor/cargo", "1.98.1");
    let extended = command.with_env(&extra).map_err(|err| err.to_string())?;
    let full = extended.full_env();
    assert_eq!(&full[full.len() - extra.len()..], extra.as_slice());
    assert!(
        full.iter().any(|(key, _)| key == "MISE_EXEC_AUTO_INSTALL"),
        "verification disable survives extension: {full:?}"
    );
    Ok(())
}

#[test]
fn credential_strip_set_is_exact() {
    assert_eq!(
        CREDENTIAL_ENV_KEYS,
        [
            "MISE_GITHUB_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "ACTIONS_RUNTIME_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "CARGO_REGISTRY_TOKEN",
            "NPM_TOKEN",
            "NODE_AUTH_TOKEN",
        ]
    );
}

#[test]
fn credential_pattern_catches_registry_and_token_variants() {
    for denied in [
        "CARGO_REGISTRIES_ACME_TOKEN",
        "NPM_TOKEN",
        "MY_REGISTRY_TOKEN",
    ] {
        assert!(is_denied_credential_key(denied), "{denied} must match");
        assert!(is_reserved_env_key(denied), "{denied} must be reserved");
    }
    for clean in ["MISE_RUSTUP_HOME", "TOKEN_COUNT", "CARGO_REGISTRIES"] {
        assert!(!is_denied_credential_key(clean), "{clean} must pass");
    }
}

#[test]
fn oidc_token_pair_never_reaches_task_env() -> Result<(), String> {
    for key in [
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
    ] {
        assert!(CREDENTIAL_ENV_KEYS.contains(&key), "{key} in strip set");
        assert!(is_reserved_env_key(key), "{key} reserved");
        let pair = [(OsString::from(key), OsString::from("sentinel"))];
        let exec = IsolatedCommand::mise_exec(
            &["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()],
            &[OsString::from("cargo")],
        )
        .map_err(|err| err.to_string())?;
        assert!(
            matches!(
                exec.with_env(&pair),
                Err(MiseError::InvalidStepInput { .. })
            ),
            "{key} must fail loud via with_env"
        );
        let declared = vec![(OsString::from(key), OsString::from("sentinel"))];
        assert!(
            IsolatedCommand::repo_task("sh", Vec::new(), &declared).is_err(),
            "{key} must fail loud via repo_task"
        );
    }
    let task = IsolatedCommand::repo_task("sh", Vec::new(), &[]).map_err(|err| err.to_string())?;
    assert!(
        !task.full_env().iter().any(|(key, _)| {
            key == "ACTIONS_ID_TOKEN_REQUEST_TOKEN" || key == "ACTIONS_ID_TOKEN_REQUEST_URL"
        }),
        "task env carries no OIDC pair: {:?}",
        task.full_env()
    );
    Ok(())
}

#[test]
fn endpoint_selectors_are_reserved_and_unsettable() -> Result<(), String> {
    use velnor_actions_mise::ENDPOINT_ENV_KEYS;
    use velnor_actions_mise::command::is_denied_endpoint_key;
    assert_eq!(ENDPOINT_ENV_KEYS, ["GH_HOST", "GH_CONFIG_DIR"]);
    for key in ENDPOINT_ENV_KEYS {
        assert!(is_denied_endpoint_key(key), "{key} must match");
        assert!(is_reserved_env_key(key), "{key} reserved");
        let pair = [(OsString::from(key), OsString::from("sentinel"))];
        let exec = IsolatedCommand::mise_exec(
            &["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()],
            &[OsString::from("cargo")],
        )
        .map_err(|err| err.to_string())?;
        assert!(
            matches!(
                exec.with_env(&pair),
                Err(MiseError::InvalidStepInput { .. })
            ),
            "{key} must fail loud via with_env"
        );
        let declared = vec![(OsString::from(key), OsString::from("sentinel"))];
        assert!(
            IsolatedCommand::repo_task("sh", Vec::new(), &declared).is_err(),
            "{key} must fail loud via repo_task"
        );
    }
    Ok(())
}

#[test]
fn mise_prefix_is_reserved_except_owned_homes() -> Result<(), String> {
    for key in [
        "MISE_ENV",
        "MISE_CONFIG_FILE",
        "MISE_TRUSTED_CONFIG_PATHS",
        "MISE_DATA_DIR",
        "MISE_NO_CONFIG",
        "MISE_LOCKFILE",
        "MISE_SUDO",
        "MISE_OWNED_CARGO_WRAPPER",
        "MISE_OWNED_CARGO_WRAPPER_SHA256",
    ] {
        assert!(is_reserved_env_key(key), "{key} reserved");
        let pair = [(OsString::from(key), OsString::from("sentinel"))];
        let exec = IsolatedCommand::mise_exec(
            &["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()],
            &[OsString::from("cargo")],
        )
        .map_err(|err| err.to_string())?;
        assert!(
            matches!(
                exec.with_env(&pair),
                Err(MiseError::InvalidStepInput { .. })
            ),
            "{key} must fail loud via with_env"
        );
        let declared = vec![(OsString::from(key), OsString::from("sentinel"))];
        assert!(
            IsolatedCommand::repo_task("sh", Vec::new(), &declared).is_err(),
            "{key} must fail loud via repo_task"
        );
    }
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME"] {
        assert!(!is_reserved_env_key(key), "{key} stays allowed");
    }
    Ok(())
}

#[test]
fn caller_cannot_supply_a_complete_owned_wrapper_authority_pair() {
    let declared = [
        (
            OsString::from("MISE_OWNED_CARGO_WRAPPER"),
            OsString::from("/tmp/velnor/mise/installs/mr-boxington/mbx"),
        ),
        (
            OsString::from("MISE_OWNED_CARGO_WRAPPER_SHA256"),
            OsString::from("a".repeat(64)),
        ),
    ];
    assert!(matches!(
        IsolatedCommand::repo_task("sh", Vec::new(), &declared),
        Err(MiseError::InvalidStepInput { field, value })
            if field == "MISE_OWNED_CARGO_WRAPPER" && value == "reserved_env_key"
    ));
}

#[test]
fn caller_cannot_set_rustup_auto_install() -> Result<(), String> {
    assert!(is_reserved_env_key("RUSTUP_AUTO_INSTALL"));
    let exec = IsolatedCommand::mise_exec(
        &["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()],
        &[OsString::from("cargo")],
    )
    .map_err(|err| err.to_string())?;
    for value in ["0", "1", "false", ""] {
        let declared = [(OsString::from("RUSTUP_AUTO_INSTALL"), OsString::from(value))];
        for result in [
            exec.clone().with_env(&declared),
            IsolatedCommand::repo_task("sh", Vec::new(), &declared),
        ] {
            assert!(matches!(
                result,
                Err(MiseError::InvalidStepInput { field, value })
                    if field == "RUSTUP_AUTO_INSTALL" && value == "reserved_env_key"
            ));
        }
    }
    Ok(())
}

#[test]
fn subcommand_allowlist_is_exec_install_run() {
    assert_eq!(ALLOWED_MISE_SUBCOMMANDS, ["exec", "install", "run"]);
    for allowed in ["exec", "install", "run"] {
        assert!(
            is_allowed_mise_subcommand(allowed),
            "{allowed} must be allowed"
        );
    }
    for rejected in [
        "use",
        "lock",
        "upgrade",
        "self-update",
        "settings",
        "config",
        "env",
        "tasks",
        "trust",
        "prune",
        "uninstall",
        "",
    ] {
        assert!(
            !is_allowed_mise_subcommand(rejected),
            "{rejected} must be rejected"
        );
    }
}

#[test]
fn every_built_mise_subcommand_is_allowlisted() -> Result<(), String> {
    let exec = IsolatedCommand::mise_exec(
        &["rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned()],
        &[OsString::from("cargo")],
    )
    .map_err(|err| err.to_string())?;
    let install = IsolatedCommand::mise_install(&[
        "rust[profile=minimal,components=clippy,rustfmt]@1.98.1".to_owned(),
    ])
    .map_err(|err| err.to_string())?;
    for argv in [exec.argv(), install.argv()] {
        assert_eq!(argv[0], OsString::from("mise"));
        let subcommand = argv
            .iter()
            .skip(1)
            .find(|arg| !arg.to_string_lossy().starts_with("--"))
            .map(|arg| arg.to_string_lossy().into_owned())
            .ok_or("missing subcommand")?;
        assert!(
            is_allowed_mise_subcommand(&subcommand),
            "unallowlisted subcommand: {subcommand}"
        );
    }
    Ok(())
}
