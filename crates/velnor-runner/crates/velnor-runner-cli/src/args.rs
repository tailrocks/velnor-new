//! Command tree. The binary name is `velnor-host`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// macOS scale-set controller.
#[derive(Debug, Parser)]
#[command(name = "velnor-host", version, about)]
pub struct Cli {
    /// Controller state directory. Defaults to Application Support.
    #[arg(long, global = true)]
    pub state: Option<PathBuf>,
    /// Command.
    #[command(subcommand)]
    pub command: Command,
}

/// Operator commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Bind one repository and scale set. The token is read from stdin and is not a flag.
    Connect {
        /// Stable ID for this connection.
        #[arg(long)]
        connection_id: String,
        /// `owner/name`.
        #[arg(long)]
        repo: String,
        /// Immutable GitHub repository ID.
        #[arg(long)]
        repository_id: i64,
        /// Scale Set name.
        #[arg(long)]
        scale_set: String,
        /// Exact registered label. Repeat for every label.
        #[arg(long = "scale-set-label", required = true)]
        scale_set_labels: Vec<String>,
        /// Source authorization policy: `repository` or `branch-restricted`.
        #[arg(
            long = "source-policy-mode",
            required = true,
            value_parser = ["repository", "branch-restricted"]
        )]
        source_policy_mode: String,
        /// Allowed event. Repeat for every event.
        #[arg(
            long = "event",
            required = true,
            value_parser = ["pull_request", "push", "merge_group", "schedule", "workflow_dispatch", "release"]
        )]
        events: Vec<String>,
        /// Allowed branch ref. Repeat for every branch.
        #[arg(long = "branch")]
        branches: Vec<String>,
        /// Allowed tag prefix. Repeat for every prefix.
        #[arg(long = "tag-prefix")]
        tag_prefixes: Vec<String>,
        /// Pull request rule: NUMBER:BASE_REPOSITORY_ID:BASE_BRANCH:HEAD_REPOSITORY_ID:HEAD_BRANCH. Use `*` for any number.
        #[arg(long = "pull-request-rule")]
        pull_request_rules: Vec<String>,
        /// Container platform.
        #[arg(long)]
        platform: String,
        /// Total permits. Defaults to 1.
        #[arg(long)]
        max_jobs: Option<u32>,
        /// Runner CPU allocation in millicores.
        #[arg(long)]
        runner_cpu_millicores: u64,
        /// Runner memory limit in bytes.
        #[arg(long)]
        runner_memory_bytes: u64,
        /// Private `DinD` CPU allocation in millicores.
        #[arg(long)]
        dind_cpu_millicores: u64,
        /// Private `DinD` memory limit in bytes.
        #[arg(long)]
        dind_memory_bytes: u64,
        /// Host memory reserved for operational overhead in bytes.
        #[arg(long)]
        operational_memory_reserve_bytes: u64,
        /// Docker context name.
        #[arg(long)]
        docker_context: Option<String>,
        /// Unix socket endpoint.
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Print readiness.
    Status {
        /// JSON object.
        #[arg(long)]
        json: bool,
    },
    /// Read-only checks.
    Doctor {
        /// Allow a short-lived owned probe.
        #[arg(long)]
        probe: bool,
    },
    /// Print the log path.
    Logs {
        /// Show new service log output.
        #[arg(long)]
        follow: bool,
    },
    /// Stop new admissions.
    Drain {
        /// Wait up to 300 seconds for active work to finish.
        #[arg(long)]
        wait: bool,
    },
    /// Allow admissions again.
    Resume,
    /// `LaunchAgent` lifecycle.
    Service {
        /// install, start, stop, or uninstall.
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Foreground daemon.
    Daemon {
        /// run.
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Fail-closed lane comparison.
    Compare {
        /// Repository.
        #[arg(long)]
        repo: String,
        /// Workflow run id.
        #[arg(long)]
        run_id: String,
        /// Run attempt.
        #[arg(long)]
        attempt: u64,
        /// Directory of expected, observed, and census JSON.
        #[arg(long, value_name = "DIR")]
        evidence: Option<PathBuf>,
    },
    /// Drain one repository connection.
    Disconnect {
        /// Stable ID for this connection.
        #[arg(long)]
        connection_id: String,
        /// Wait up to 300 seconds for active work. Then remove the connection.
        #[arg(long)]
        wait: bool,
    },
}

/// `service` actions.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum ServiceAction {
    /// Write the `LaunchAgent` plist.
    Install,
    /// Start is launchd's job. This process does not double-fork.
    Start,
    /// Stop the agent.
    Stop,
    /// Remove the plist.
    Uninstall,
}

/// `daemon` actions.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum DaemonAction {
    /// Run in the foreground.
    Run,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::Parser;

    use super::{Cli, Command, ServiceAction};

    #[test]
    fn state_is_global_before_or_after_the_service_command() -> Result<(), String> {
        for arguments in [
            vec![
                "velnor-host",
                "--state",
                "/tmp/velnor-state",
                "service",
                "install",
            ],
            vec![
                "velnor-host",
                "service",
                "install",
                "--state",
                "/tmp/velnor-state",
            ],
        ] {
            let cli = Cli::try_parse_from(arguments).map_err(|error| error.to_string())?;
            if cli.state.as_deref() != Some(Path::new("/tmp/velnor-state"))
                || !matches!(
                    cli.command,
                    Command::Service {
                        action: ServiceAction::Install
                    }
                )
            {
                return Err("global state path was not parsed".to_owned());
            }
        }
        Ok(())
    }

    #[test]
    fn drain_wait_is_parsed_with_the_selected_state() -> Result<(), String> {
        let cli = Cli::try_parse_from([
            "velnor-host",
            "drain",
            "--wait",
            "--state",
            "/tmp/velnor-state",
        ])
        .map_err(|error| error.to_string())?;
        if cli.state.as_deref() != Some(Path::new("/tmp/velnor-state"))
            || !matches!(cli.command, Command::Drain { wait: true })
        {
            return Err("drain did not use the selected state".to_owned());
        }
        Ok(())
    }

    #[test]
    fn disconnect_requires_a_connection_id() -> Result<(), String> {
        if Cli::try_parse_from(["velnor-host", "disconnect"]).is_ok() {
            return Err("disconnect accepted no connection ID".to_owned());
        }
        let cli = Cli::try_parse_from([
            "velnor-host",
            "disconnect",
            "--connection-id",
            "example-repo",
            "--wait",
        ])
        .map_err(|error| error.to_string())?;
        if !matches!(
            cli.command,
            Command::Disconnect {
                connection_id,
                wait: true
            } if connection_id == "example-repo"
        ) {
            return Err("disconnect lost its connection ID or wait flag".to_owned());
        }
        Ok(())
    }
}
