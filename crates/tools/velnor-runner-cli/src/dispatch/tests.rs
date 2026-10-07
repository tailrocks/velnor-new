//! Dispatch-level command tests.

mod connect_tests;

use std::path::Path;

use clap::Parser;

use super::{config_path, selected_config_path};
use crate::args::Cli;

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
