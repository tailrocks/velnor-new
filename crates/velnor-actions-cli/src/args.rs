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

/// Public commands: init, plan, generate, and config.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Create `.velnor/config.toml` in the Git working tree.
    Init,
    /// Print the deterministic plan report to stdout.
    Plan {
        /// Override configured execution mode: hosted, scale-set, or both.
        #[arg(long = "mode", value_name = "MODE")]
        mode: Option<String>,
    },
    /// Render and write the `.github` tree.
    Generate {
        /// Preview root; writes `PATH/.github` without touching the repository.
        /// Choose a unique directory under /tmp or runner temporary storage.
        #[arg(long = "output-dir", value_name = "PATH")]
        output_dir: Option<PathBuf>,
        /// Override configured execution mode: hosted, scale-set, or both.
        #[arg(long = "mode", value_name = "MODE")]
        mode: Option<String>,
    },
    /// Read or rewrite `.velnor/config.toml`.
    Config {
        /// Config operation.
        #[command(subcommand)]
        action: ConfigCommand,
    },
}

/// Config operations.
#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// Print the schema 2 config. `--write` saves that same text.
    Migrate {
        /// Target schema. Only 2 is accepted.
        #[arg(long = "to")]
        to: u32,
        /// Persist the printed config.
        #[arg(long = "write")]
        write: bool,
    },
}
