//! Preflight route-selection cases.
use std::ffi::OsString;
use velnor_actions_mise_catalog::{RouteDriver, ToolCatalog, select_route};
use velnor_actions_mise_core::MiseError;

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

#[test]
fn cargo_proof_pins_exact_toolchain_without_wrapper() -> Result<(), String> {
    let selection = select_route(&pinned(), RouteDriver::Cargo, "cargo-1", "gen-7")
        .map_err(|err| err.to_string())?;
    assert_eq!(selection.driver(), RouteDriver::Cargo);
    assert_eq!(selection.identity_specs(), &["rust@1.98.1".to_owned()]);
    assert_eq!(selection.probe_specs(), &["rust@1.98.1".to_owned()]);
    let invocation = selection.invocation(&pinned());
    assert_eq!(
        invocation,
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.98.1",
            "--",
            "cargo",
            "--version",
        ]
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>()
    );
    assert!(
        !invocation
            .iter()
            .any(|arg| arg.to_string_lossy().contains("boxington")),
        "cargo proof must show no MBX wrapper: {invocation:?}"
    );
    assert!(velnor_actions_contract::is_valid_digest(
        selection.cache_format_id()
    ));
    Ok(())
}

#[test]
fn mbx_route_separates_action_identity_from_mise_probe() -> Result<(), String> {
    let selection = select_route(&pinned(), RouteDriver::Mbx, "mbx-obj-3", "gen-9")
        .map_err(|err| err.to_string())?;
    assert_eq!(selection.driver(), RouteDriver::Mbx);
    assert_eq!(
        selection.identity_specs(),
        &["rust@1.98.1".to_owned(), "mr-boxington@1.21.1".to_owned()]
    );
    assert_eq!(selection.probe_specs(), &["rust@1.98.1".to_owned()]);
    let invocation = selection.invocation(&pinned());
    assert!(!invocation.iter().any(|arg| arg == "mr-boxington@1.21.1"));
    assert_eq!(
        invocation.last(),
        Some(&OsString::from("--version")),
        "identity probe invocation: {invocation:?}"
    );
    assert_eq!(invocation[0], OsString::from("mise"));
    assert!(velnor_actions_contract::is_valid_digest(
        selection.cache_format_id()
    ));
    Ok(())
}

#[test]
fn unreportable_format_fails_without_guessing() {
    for (format, generation) in [("", "gen-9"), ("mbx-obj-3", ""), ("", "")] {
        let err = select_route(&pinned(), RouteDriver::Mbx, format, generation)
            .expect_err("unreportable MBX format must fail");
        assert!(
            err.to_string().contains("format_unreportable"),
            "precise miss reason: {err}"
        );
    }
    let err = select_route(&pinned(), RouteDriver::Cargo, "", "gen-7")
        .expect_err("unreportable cargo format must fail");
    assert!(err.to_string().contains("format_unreportable"));
    assert!(matches!(err, MiseError::Contract { .. }));
}

#[test]
fn route_selections_are_deterministic() -> Result<(), String> {
    let first = select_route(&pinned(), RouteDriver::Mbx, "mbx-obj-3", "gen-9")
        .map_err(|err| err.to_string())?;
    let second = select_route(&pinned(), RouteDriver::Mbx, "mbx-obj-3", "gen-9")
        .map_err(|err| err.to_string())?;
    assert_eq!(first, second);
    let other = select_route(&pinned(), RouteDriver::Mbx, "mbx-obj-4", "gen-9")
        .map_err(|err| err.to_string())?;
    assert_ne!(first.cache_format_id(), other.cache_format_id());
    Ok(())
}

#[test]
fn proof_command_matches_invocation() -> Result<(), String> {
    let selection = select_route(&pinned(), RouteDriver::Cargo, "cargo-1", "gen-7")
        .map_err(|err| err.to_string())?;
    let command = selection
        .command(&pinned())
        .map_err(|err| err.to_string())?;
    assert_eq!(command.argv(), selection.invocation(&pinned()));
    assert_eq!(command.program(), "mise");
    Ok(())
}

#[test]
fn compile_driver_spelling_maps_to_route() {
    assert_eq!(
        RouteDriver::from_compile_driver("cargo"),
        Some(RouteDriver::Cargo)
    );
    assert_eq!(
        RouteDriver::from_compile_driver("mbx"),
        Some(RouteDriver::Mbx)
    );
    assert_eq!(RouteDriver::Cargo.program(), "cargo");
    assert_eq!(RouteDriver::Mbx.program(), "mbx");
    for unknown in ["", "rustc", "latest", "stable", "cargo ", "MBX"] {
        assert_eq!(
            RouteDriver::from_compile_driver(unknown),
            None,
            "{unknown} must resolve to no route"
        );
    }
}
