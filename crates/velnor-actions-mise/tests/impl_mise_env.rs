//! Toolchain environment and subcommand allowlist cases.
use std::ffi::OsString;
use velnor_actions_mise::{
    ALLOWED_MISE_SUBCOMMANDS, GitRequest, IsolatedCommand, MISE_CARGO_HOME_ENV,
    MISE_RUSTUP_HOME_ENV, NO_AUTO_INSTALL_ENV, RUSTUP_TOOLCHAIN_ENV, ToolCatalog,
    is_allowed_mise_subcommand, toolchain_env,
};

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
        "1.97.0", "1.19.0", "2.101.0", "1.7.12", "0.11.0", "1.30.1", "0.9.146",
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(catalog.rustup_toolchain(), "1.97.0");
    Ok(())
}

#[test]
fn toolchain_env_triple_is_exact() {
    assert_eq!(
        toolchain_env("/velnor/rustup", "/velnor/cargo", "1.98.1"),
        vec![
            (
                OsString::from("MISE_RUSTUP_HOME"),
                OsString::from("/velnor/rustup")
            ),
            (
                OsString::from("MISE_CARGO_HOME"),
                OsString::from("/velnor/cargo")
            ),
            (OsString::from("RUSTUP_TOOLCHAIN"), OsString::from("1.98.1")),
        ]
    );
}

#[test]
fn exec_disables_implicit_install() -> Result<(), String> {
    let command = IsolatedCommand::mise_exec(
        &["rust@1.98.1".to_owned()],
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
        "shared overlay stays the quartet: {overlay:?}"
    );
    Ok(())
}

#[test]
fn install_and_direct_keep_install_enabled() -> Result<(), String> {
    let install = IsolatedCommand::mise_install(&["rust@1.98.1".to_owned()])
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
        &["rust@1.98.1".to_owned()],
        &[OsString::from("cargo"), OsString::from("--version")],
    )
    .map_err(|err| err.to_string())?;
    let extra = toolchain_env("/velnor/rustup", "/velnor/cargo", "1.98.1");
    let extended = command.with_env(&extra);
    let full = extended.full_env();
    assert_eq!(&full[full.len() - 3..], extra.as_slice());
    assert!(
        full.iter().any(|(key, _)| key == "MISE_EXEC_AUTO_INSTALL"),
        "verification disable survives extension: {full:?}"
    );
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
    let exec = IsolatedCommand::mise_exec(&["rust@1.98.1".to_owned()], &[OsString::from("cargo")])
        .map_err(|err| err.to_string())?;
    let install = IsolatedCommand::mise_install(&["rust@1.98.1".to_owned()])
        .map_err(|err| err.to_string())?;
    for argv in [exec.argv(), install.argv()] {
        assert_eq!(argv[0], OsString::from("mise"));
        let subcommand = argv[4].to_string_lossy().into_owned();
        assert!(
            is_allowed_mise_subcommand(&subcommand),
            "unallowlisted subcommand: {subcommand}"
        );
    }
    Ok(())
}
