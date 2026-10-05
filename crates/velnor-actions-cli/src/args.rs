//! Clap command tree for `velnor-actions`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Velnor Actions: generate GitHub Actions workflows from repository state.
#[derive(Debug, Parser)]
#[command(name = "velnor-actions", version, about)]
pub(crate) struct Cli {
    /// Command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Public command tree: init, plan, generate, freshness, and config.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Create `.velnor/config.toml` in the Git working tree.
    Init,
    /// Print the deterministic plan report to stdout.
    Plan,
    /// Render and write the `.github` tree.
    Generate {
        /// Preview root; writes `PATH/.github` without touching the repository.
        /// Choose a unique directory under /tmp or runner temporary storage.
        #[arg(long = "output-dir", value_name = "PATH")]
        output_dir: Option<PathBuf>,
        /// Dispatch mode override: `hosted`, `scale-set`, or `both`.
        #[arg(long = "mode", value_name = "MODE")]
        mode: Option<String>,
        /// Preview only the fixed published Foundation native qualification workflow.
        /// Requires canonical generator identity and an empty external destination.
        #[arg(long, requires = "output_dir", conflicts_with = "mode")]
        foundation_qualification_only: bool,
    },
    /// Check repository pins, policy mirrors, lock identity, and evidence.
    Freshness {
        /// Repository root to inspect.
        #[arg(long = "root", value_name = "DIR", default_value = ".")]
        root: PathBuf,
        /// Include the bounded read-only upstream probe.
        #[arg(long = "check-upstream")]
        check_upstream: bool,
        /// Run Cargo Deny advisories in addition to local policy checks.
        #[arg(long = "with-advisories")]
        with_advisories: bool,
    },
    /// Read or rewrite `.velnor/config.toml`.
    Config {
        /// Config subcommand.
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

/// Config subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// Print the schema 2 config. `--write` stores that same text.
    Migrate {
        /// Target schema. Only `2` is accepted.
        #[arg(long = "to", value_name = "SCHEMA")]
        to: u32,
        /// Persist the preview instead of only printing it.
        #[arg(long = "write")]
        write: bool,
    },
}
