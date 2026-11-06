// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

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
        #[command(flatten)]
        args: Box<ConnectArgs>,
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
    /// Host credential operations.
    Credential {
        /// Action.
        #[command(subcommand)]
        action: CredentialAction,
    },
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

/// Arguments for one repository and Scale Set binding.
#[derive(Debug, Args)]
pub struct ConnectArgs {
    /// Stable ID for this connection.
    #[arg(long)]
    pub connection_id: String,
    /// `owner/name`.
    #[arg(long)]
    pub repo: String,
    /// Immutable GitHub repository ID.
    #[arg(long)]
    pub repository_id: i64,
    /// Give the path to the reviewed host-policy JSON object for this connection.
    #[arg(long, value_name = "FILE")]
    pub job_authority_policy: Option<PathBuf>,
    /// Give the path to the generator inventory in JSON format. Also supply `--job-authority-generator-source-sha`, `--job-authority-generator-artifact-sha256`, and `--job-authority-generation`.
    #[arg(long, value_name = "FILE")]
    pub job_authority_inventory: Option<PathBuf>,
    /// Use the reviewed source commit for the inventory generator. Enter 40 lowercase hexadecimal digits.
    #[arg(long)]
    pub job_authority_generator_source_sha: Option<String>,
    /// Use the reviewed SHA-256 digest of the inventory generator executable. Enter 64 lowercase hexadecimal digits.
    #[arg(long)]
    pub job_authority_generator_artifact_sha256: Option<String>,
    /// Set the generation number for this authority policy.
    #[arg(long)]
    pub job_authority_generation: Option<u64>,
    /// Scale Set name.
    #[arg(long)]
    pub scale_set: String,
    /// Exact registered label. Repeat for every label.
    #[arg(long = "scale-set-label", required = true)]
    pub scale_set_labels: Vec<String>,
    /// Source authorization policy: `repository` or `branch-restricted`.
    #[arg(
        long = "source-policy-mode",
        required = true,
        value_parser = ["repository", "branch-restricted"]
    )]
    pub source_policy_mode: String,
    /// Allowed event. Repeat for every event.
    #[arg(
        long = "event",
        required = true,
        value_parser = ["pull_request", "push", "merge_group", "schedule", "workflow_dispatch", "release"]
    )]
    pub events: Vec<String>,
    /// Allowed branch ref. Repeat for every branch.
    #[arg(long = "branch")]
    pub branches: Vec<String>,
    /// Allowed tag prefix. Repeat for every prefix.
    #[arg(long = "tag-prefix")]
    pub tag_prefixes: Vec<String>,
    /// Pull request rule: `NUMBER:BASE_REPOSITORY_ID:BASE_BRANCH:HEAD_REPOSITORY_ID:HEAD_BRANCH`. Use `*` for any number.
    #[arg(long = "pull-request-rule")]
    pub pull_request_rules: Vec<String>,
    /// Container platform.
    #[arg(long)]
    pub platform: String,
    /// Total permits. Defaults to 1.
    #[arg(long)]
    pub max_jobs: Option<u32>,
    /// Runner CPU allocation in millicores.
    #[arg(long)]
    pub runner_cpu_millicores: u64,
    /// Runner memory limit in bytes.
    #[arg(long)]
    pub runner_memory_bytes: u64,
    /// Private `DinD` CPU allocation in millicores.
    #[arg(long)]
    pub dind_cpu_millicores: u64,
    /// Private `DinD` memory limit in bytes.
    #[arg(long)]
    pub dind_memory_bytes: u64,
    /// Host memory reserved for operational overhead in bytes.
    #[arg(long)]
    pub operational_memory_reserve_bytes: u64,
    /// Docker context name.
    #[arg(long)]
    pub docker_context: Option<String>,
    /// Specify the exact Unix socket endpoint for the selected Docker engine.
    #[arg(long, required = true)]
    pub endpoint: String,
}

/// Host credential actions.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum CredentialAction {
    /// Read the release credential from stdin. Store it in Keychain.
    ImportReleaseRead,
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
