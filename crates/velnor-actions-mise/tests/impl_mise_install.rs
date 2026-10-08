//! Bootstrap install request cases.
use std::ffi::{OsStr, OsString};
use velnor_actions_mise::{MiseError, MiseInstall, PinnedTool, PinnedToolExec, ToolCatalog};

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

fn strings(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn install_argv_is_byte_exact() -> Result<(), String> {
    let request = MiseInstall::new(vec![PinnedTool::Rust, PinnedTool::MrBoxington])
        .map_err(|err| err.to_string())?;
    assert_eq!(
        request.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            "rust@1.98.1",
            "mr-boxington@1.21.1",
        ])
    );
    Ok(())
}

#[test]
fn install_carries_specs_only() -> Result<(), String> {
    let request = MiseInstall::new(PinnedTool::ALL.to_vec()).map_err(|err| err.to_string())?;
    let argv = request.argv(&pinned());
    assert_eq!(argv[4], OsString::from("install"));
    assert!(
        !argv.iter().any(|arg| arg == "--"),
        "install takes specs, never a payload separator: {argv:?}"
    );
    for spec in [
        "rust@1.98.1",
        "mr-boxington@1.21.1",
        "gh@2.102.0",
        "actionlint@1.7.12",
        "shellcheck@0.11.0",
        "zizmor@1.30.1",
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.148",
        "opentofu@1.13.1",
    ] {
        assert!(argv.iter().any(|arg| arg == spec), "missing spec: {spec}");
    }
    Ok(())
}

#[test]
fn install_empty_toolchain_rejected() {
    assert!(matches!(
        MiseInstall::new(Vec::new()),
        Err(MiseError::EmptyToolchain)
    ));
}

#[test]
fn install_command_matches_argv_and_keeps_install_enabled() -> Result<(), String> {
    let request = MiseInstall::new(vec![PinnedTool::Rust]).map_err(|err| err.to_string())?;
    let command = request.command(&pinned()).map_err(|err| err.to_string())?;
    assert_eq!(command.argv(), request.argv(&pinned()));
    assert_eq!(command.program(), "mise");
    let env = command.full_env();
    for present in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
    ] {
        assert!(
            env.iter().any(|(key, _)| key == present),
            "isolated install keeps {present}: {env:?}"
        );
    }
    for blocked in ["MISE_AUTO_INSTALL", "MISE_EXEC_AUTO_INSTALL"] {
        assert!(
            !env.iter().any(|(key, _)| key == blocked),
            "explicit install must stay enabled: {blocked}"
        );
    }
    Ok(())
}

#[test]
fn install_specs_come_only_from_catalog() -> Result<(), String> {
    let catalog = ToolCatalog::new(
        "1.97.0", "1.18.0", "2.100.0", "1.7.11", "0.10.0", "1.30.0", "0.9.145", "1.13.0",
    )
    .map_err(|err| err.to_string())?;
    let request = MiseInstall::new(vec![PinnedTool::Rust, PinnedTool::MrBoxington])
        .map_err(|err| err.to_string())?;
    let argv = request.argv(&catalog);
    assert!(argv.iter().any(|arg| arg == "rust@1.97.0"));
    assert!(argv.iter().any(|arg| arg == "mr-boxington@1.18.0"));
    assert!(
        !argv.iter().any(|arg| arg == "rust@1.98.1"),
        "no pinned fallback may leak in: {argv:?}"
    );
    Ok(())
}

#[test]
fn cargo_profile_install_mentions_no_mbx() -> Result<(), String> {
    let request = MiseInstall::new(vec![PinnedTool::Rust]).map_err(|err| err.to_string())?;
    let argv = request.argv(&pinned());
    assert!(
        !argv.iter().any(
            |arg| arg.to_string_lossy().contains("boxington") || arg.to_string_lossy() == "mbx"
        ),
        "cargo profiles must not install MBX: {argv:?}"
    );
    Ok(())
}

#[test]
fn cargo_profile_exec_invokes_no_mbx() -> Result<(), String> {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        OsStr::new("cargo"),
        strings(&["--version"]),
    )
    .map_err(|err| err.to_string())?;
    let argv = exec.argv(&pinned());
    assert!(
        !argv.iter().any(
            |arg| arg.to_string_lossy().contains("boxington") || arg.to_string_lossy() == "mbx"
        ),
        "cargo profiles must not invoke MBX: {argv:?}"
    );
    Ok(())
}

#[test]
fn mbx_install_carries_exact_mbx_spec() -> Result<(), String> {
    let request = MiseInstall::new(vec![PinnedTool::Rust, PinnedTool::MrBoxington])
        .map_err(|err| err.to_string())?;
    let argv = request.argv(&pinned());
    assert!(argv.iter().any(|arg| arg == "mr-boxington@1.21.1"));
    Ok(())
}
