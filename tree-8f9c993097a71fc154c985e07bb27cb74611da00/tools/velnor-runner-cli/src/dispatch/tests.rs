//! Dispatch-level command tests.

mod connect_common;
mod connect_guard_tests;
mod connect_tests;
mod doctor_tests;

use std::path::Path;

use clap::Parser;

use super::{config_path, disconnect_for_os, requested_wait, selected_config_path};
use crate::args::Cli;

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DISCONNECT_STATE: AtomicUsize = AtomicUsize::new(0);

fn disconnect_state_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-disconnect-{}-{}",
        std::process::id(),
        NEXT_DISCONNECT_STATE.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn explicit_config_path_is_preserved() {
    let selected = config_path(
        Some(Path::new("/tmp/velnor-host-test.toml")),
        Path::new("/var/lib/velnor-host"),
    );
    assert_eq!(selected, Path::new("/tmp/velnor-host-test.toml"));
}

#[test]
fn daemon_uses_the_global_config_override() -> Result<(), String> {
    let cli = Cli::try_parse_from([
        "velnor-host",
        "--config",
        "/etc/velnor-host/operator.toml",
        "daemon",
        "run",
    ])
    .map_err(|error| error.to_string())?;
    let state = Path::new("/var/lib/velnor-host");
    assert_eq!(
        selected_config_path(&cli, state),
        Path::new("/etc/velnor-host/operator.toml")
    );
    assert!(matches!(cli.command, crate::args::Command::Daemon { .. }));
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_default_config_uses_the_package_owned_path() {
    assert_eq!(
        config_path(None, Path::new("/var/lib/velnor-host")),
        Path::new(velnor_runner_host::LINUX_CONFIG_PATH)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_default_config_stays_under_application_support() {
    assert_eq!(
        config_path(
            None,
            Path::new("/Users/example/Library/Application Support/Velnor")
        ),
        Path::new("/Users/example/Library/Application Support/Velnor/host.toml")
    );
}

#[test]
fn disconnect_requires_explicit_drain_and_wait() {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(&state, false, false, None, "macos"),
        ExitCode::from(2)
    );
    assert!(!state.exists());
}

#[test]
fn macos_disconnect_records_only_the_legacy_marker_and_never_claims_disconnect()
-> Result<(), String> {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(&state, true, true, Some(30), "macos"),
        ExitCode::from(1)
    );
    let marker = std::fs::read(state.join("drain")).map_err(|error| error.to_string())?;
    assert_eq!(marker, b"1");
    assert_eq!(requested_wait(Some(30)), "the requested 30-second");
    assert_eq!(
        velnor_runner_host::disconnect_effects(velnor_runner_host::SetOwnership::Adopted, true),
        vec![velnor_runner_host::DisconnectEffect::Drain]
    );
    std::fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn linux_disconnect_wait_fails_without_remote_or_marker_effects() {
    let state = disconnect_state_path();
    assert_eq!(
        disconnect_for_os(&state, true, true, None, "linux"),
        ExitCode::from(1)
    );
    assert!(!state.exists());
    assert_eq!(requested_wait(None), "the configured-timeout");
}
