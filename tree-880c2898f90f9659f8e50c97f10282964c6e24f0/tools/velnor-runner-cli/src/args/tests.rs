use clap::Parser;

use super::{Cli, Command};

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
fn connect_requires_explicit_linux_profile_scope_group_and_trust() -> Result<(), String> {
    let cli = Cli::try_parse_from([
        "velnor-host",
        "connect",
        "--repo",
        "ChainArgos/java-monorepo",
        "--scale-set",
        "ubuntu-24.04-scale-set",
        "--platform",
        "linux/amd64",
        "--host-platform",
        "linux",
        "--registration-scope",
        "repository",
        "--runner-group-id",
        "1",
        "--runner-group-name",
        "Default",
        "--allow-event",
        "push",
        "--allow-event",
        "pull_request",
        "--allow-workflow-path",
        ".github/workflows/ci.yml",
        "--image-profile",
        "ubuntu-24.04-amd64",
        "--max-jobs",
        "4",
        "--drain-timeout-secs",
        "900",
    ])
    .map_err(|error| error.to_string())?;
    assert!(matches!(cli.command, Command::Connect(_)));
    Ok(())
}

#[test]
fn connect_preserves_exact_repeatable_workflow_paths() -> Result<(), String> {
    let cli = Cli::try_parse_from([
        "velnor-host",
        "connect",
        "--repo",
        "ChainArgos/java-monorepo",
        "--scale-set",
        "ubuntu-24.04-scale-set",
        "--platform",
        "linux/amd64",
        "--host-platform",
        "linux",
        "--registration-scope",
        "repository",
        "--runner-group-id",
        "1",
        "--runner-group-name",
        "Default",
        "--allow-event",
        "push",
        "--allow-workflow-path",
        ".github/workflows/ci.yml",
        "--allow-workflow-path",
        ".github/workflows/qualification.yml",
        "--image-profile",
        "ubuntu-24.04-amd64",
        "--max-jobs",
        "2",
        "--drain-timeout-secs",
        "600",
    ])
    .map_err(|error| error.to_string())?;
    let Command::Connect(connect) = cli.command else {
        return Err("expected connect command".to_owned());
    };
    assert_eq!(
        connect.allowed_workflow_paths,
        [
            ".github/workflows/ci.yml".to_owned(),
            ".github/workflows/qualification.yml".to_owned()
        ]
    );
    assert_eq!(connect.drain_timeout_secs, Some(600));
    Ok(())
}

#[test]
fn connect_keeps_the_original_repository_scale_set_and_container_options() -> Result<(), String> {
    let cli = Cli::try_parse_from([
        "velnor-host",
        "connect",
        "--repo",
        "tailrocks/velnor-new",
        "--scale-set",
        "ubuntu-26.04-scale-set",
        "--platform",
        "linux/amd64",
    ])
    .map_err(|error| error.to_string())?;
    assert!(matches!(cli.command, Command::Connect(_)));
    Ok(())
}
