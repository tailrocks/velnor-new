//! Command tree. The binary name is `velnor-host`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Rust Scale Set controller for the selected host platform.
#[derive(Debug, Parser)]
#[command(name = "velnor-host", version, about)]
pub struct Cli {
    /// Controller state directory. Defaults are host-platform specific.
    #[arg(long, global = true)]
    pub state: Option<PathBuf>,
    /// Host configuration file. Defaults are host-platform specific.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Command.
    #[command(subcommand)]
    pub command: Command,
}

/// Input fields for the `connect` command.
#[derive(Debug, Args)]
pub struct ConnectArgs {
    /// `owner/name`.
    #[arg(long)]
    pub repo: String,
    /// Scale set name.
    #[arg(long)]
    pub scale_set: String,
    /// Container platform.
    #[arg(long)]
    pub platform: String,
    /// Controller backend. Omit on macOS to keep the existing default.
    #[arg(long, value_parser = ["linux", "macos"])]
    pub host_platform: Option<String>,
    /// Registration scope. Linux requires repository registration.
    #[arg(long, value_parser = ["repository"])]
    pub registration_scope: Option<String>,
    /// Exact GitHub runner group id. Required for Linux.
    #[arg(long)]
    pub runner_group_id: Option<i64>,
    /// Exact GitHub runner group name. Required for Linux.
    #[arg(long)]
    pub runner_group_name: Option<String>,
    /// Exact accepted workflow event. Linux requires one or more; repeat as needed.
    #[arg(long = "allow-event", action = clap::ArgAction::Append)]
    pub allowed_events: Vec<String>,
    /// Exact Actions workflow path, such as `.github/workflows/ci.yml`.
    /// Linux requires one or more; repeat as needed.
    #[arg(long = "allow-workflow-path", action = clap::ArgAction::Append)]
    pub allowed_workflow_paths: Vec<String>,
    /// Immutable supported runner image profile; required on Linux.
    #[arg(long)]
    pub image_profile: Option<String>,
    /// Total host-wide permits. Must be measured for Linux.
    #[arg(long)]
    pub max_jobs: Option<u32>,
    /// Operator-selected finite drain deadline in seconds. Linux requires it.
    #[arg(long)]
    pub drain_timeout_secs: Option<u64>,
    /// Docker context name.
    #[arg(long)]
    pub docker_context: Option<String>,
    /// Unix socket endpoint.
    #[arg(long)]
    pub endpoint: Option<String>,
}

/// Operator commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Bind one repository and scale set. The token is read from stdin and is not a flag.
    Connect(Box<ConnectArgs>),
    /// Print readiness.
    Status {
        /// JSON object.
        #[arg(long)]
        json: bool,
    },
    /// Read-only checks.
    Doctor {
        /// Perform a read-only Docker Engine GET /version; this does not prove controller readiness.
        #[arg(long)]
        probe: bool,
    },
    /// Read controller logs.
    Logs {
        /// Follow new log records until interrupted.
        #[arg(long)]
        follow: bool,
    },
    /// Stop new admissions.
    Drain {
        /// Wait for authoritative launch reconciliation and cleanup.
        #[arg(long)]
        wait: bool,
        /// Maximum wait in seconds. Defaults to the configured drain timeout.
        #[arg(long, value_name = "SECONDS", requires = "wait")]
        timeout_secs: Option<u64>,
    },
    /// Allow admissions again.
    Resume,
    /// Host service lifecycle (`systemd` on Linux, `LaunchAgent` on macOS).
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
        run_id: u64,
        /// Run attempt.
        #[arg(long)]
        attempt: u64,
        /// Directory of expected, observed, and census JSON.
        #[arg(long, value_name = "DIR")]
        evidence: Option<PathBuf>,
    },
    /// Drain the controller and disconnect only when set ownership is proven.
    Disconnect {
        /// Drain first.
        #[arg(long, requires = "wait")]
        drain: bool,
        /// Wait for authoritative launch reconciliation and cleanup.
        #[arg(long, requires = "drain")]
        wait: bool,
        /// Maximum wait in seconds. Defaults to the configured drain timeout.
        #[arg(long, value_name = "SECONDS", requires = "wait")]
        timeout_secs: Option<u64>,
    },
}

/// `service` actions.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum ServiceAction {
    /// Install or enable the platform service.
    Install,
    /// Start the installed service; durable drain remains set until `resume`.
    Start,
    /// Drain and stop the service.
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
mod tests;
