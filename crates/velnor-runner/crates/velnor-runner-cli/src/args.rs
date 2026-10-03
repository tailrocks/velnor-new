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
        /// `owner/name`.
        #[arg(long)]
        repo: String,
        /// Scale set name.
        #[arg(long)]
        scale_set: String,
        /// Container platform.
        #[arg(long)]
        platform: String,
        /// Total permits. Defaults to 1.
        #[arg(long)]
        max_jobs: Option<u32>,
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
        /// Follow is refused when the log is absent.
        #[arg(long)]
        follow: bool,
    },
    /// Stop new admissions.
    Drain {
        /// Wait is recorded. Running jobs are not killed.
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
    /// Drain and delete only a set this controller created.
    Disconnect {
        /// Drain first.
        #[arg(long)]
        drain: bool,
        /// Record that the caller is willing to wait.
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
