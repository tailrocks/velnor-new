use clap::Parser;

use super::{Cli, Command, ServiceAction};

#[test]
fn global_config_is_accepted_before_a_subcommand() -> Result<(), String> {
    let cli = Cli::try_parse_from([
        "velnor-host",
        "--config",
        "/etc/velnor-host/host.toml",
        "--state",
        "/var/lib/velnor-host",
        "daemon",
        "run",
    ])
    .map_err(|error| error.to_string())?;

    assert_eq!(
        cli.config.as_deref(),
        Some(std::path::Path::new("/etc/velnor-host/host.toml"))
    );
    assert_eq!(
        cli.state.as_deref(),
        Some(std::path::Path::new("/var/lib/velnor-host"))
    );
    assert!(matches!(cli.command, Command::Daemon { .. }));
    Ok(())
}

#[test]
fn drain_wait_accepts_a_bounded_timeout() -> Result<(), String> {
    let cli = Cli::try_parse_from(["velnor-host", "drain", "--wait", "--timeout-secs", "45"])
        .map_err(|error| error.to_string())?;

    assert!(matches!(
        cli.command,
        Command::Drain {
            wait: true,
            timeout_secs: Some(45)
        }
    ));
    Ok(())
}

#[test]
fn disconnect_requires_drain_and_wait_before_it_can_mutate_state() {
    assert!(Cli::try_parse_from(["velnor-host", "disconnect", "--drain"]).is_err());
    assert!(Cli::try_parse_from(["velnor-host", "disconnect", "--wait"]).is_err());
    assert!(
        Cli::try_parse_from([
            "velnor-host",
            "disconnect",
            "--drain",
            "--wait",
            "--timeout-secs",
            "30"
        ])
        .is_ok()
    );
}

#[test]
fn service_status_is_an_explicit_read_only_action() -> Result<(), String> {
    let cli = Cli::try_parse_from(["velnor-host", "service", "status"])
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        cli.command,
        Command::Service {
            action: ServiceAction::Status
        }
    ));
    Ok(())
}

#[test]
fn service_preflight_is_an_explicit_action() -> Result<(), String> {
    let cli = Cli::try_parse_from(["velnor-host", "service", "preflight"])
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        cli.command,
        Command::Service {
            action: ServiceAction::Preflight
        }
    ));
    Ok(())
}

mod connect_args_tests;
