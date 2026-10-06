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

/// Public command tree: init, plan, generate, and config.
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
        /// Preview only approved generator-owned tool candidate infrastructure.
        /// Requires canonical generator repository identity and source approvals.
        #[arg(long, requires = "output_dir")]
        owned_tool_candidates_only: bool,
        /// Qualify only on the exact reviewed infrastructure branch push.
        #[arg(long, requires = "owned_tool_candidates_only")]
        owned_tool_candidates_push: bool,
    },
    /// Read or rewrite `.velnor/config.toml`.
    Config {
        /// Config subcommand.
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Check a local two-target release manifest against supplied binary files.
    VerifyReleaseManifest {
        /// Release-manifest JSON file.
        #[arg(long, value_name = "PATH")]
        manifest: PathBuf,
        /// Expected source commit SHA (caller-supplied, not authenticated).
        #[arg(long, value_name = "SHA")]
        expected_source_commit: String,
        /// Local GNU/Linux x86-64 binary.
        #[arg(long, value_name = "PATH")]
        linux_x64_binary: PathBuf,
        /// Local macOS ARM64 binary.
        #[arg(long, value_name = "PATH")]
        macos_arm64_binary: PathBuf,
    },
}

impl Command {
    /// True when `generate` requests only the owned-tool candidate preview.
    pub(crate) fn is_owned_preview(&self) -> bool {
        matches!(
            self,
            Command::Generate {
                owned_tool_candidates_only: true,
                ..
            }
        )
    }
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
