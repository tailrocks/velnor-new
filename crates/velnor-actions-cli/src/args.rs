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

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::Parser;

    #[test]
    fn owned_candidates_mode_requires_explicit_preview_destination() {
        assert!(
            Cli::try_parse_from(["velnor-actions", "generate", "--owned-tool-candidates-only"])
                .is_err()
        );
        let parsed = Cli::try_parse_from([
            "velnor-actions",
            "generate",
            "--owned-tool-candidates-only",
            "--output-dir",
            "/tmp/owned-source-preview",
        ])
        .expect("closed preview mode");
        assert!(matches!(
            parsed.command,
            Command::Generate {
                output_dir: Some(_),
                owned_tool_candidates_only: true,
                owned_tool_candidates_push: false,
            }
        ));
    }

    #[test]
    fn reviewed_push_requires_source_preview_category() {
        assert!(
            Cli::try_parse_from([
                "velnor-actions",
                "generate",
                "--owned-tool-candidates-push",
                "--output-dir",
                "/tmp/owned-source-preview",
            ])
            .is_err()
        );
        let parsed = Cli::try_parse_from([
            "velnor-actions",
            "generate",
            "--owned-tool-candidates-only",
            "--owned-tool-candidates-push",
            "--output-dir",
            "/tmp/owned-source-preview",
        ])
        .expect("reviewed push category");
        assert!(matches!(
            parsed.command,
            Command::Generate {
                output_dir: Some(_),
                owned_tool_candidates_only: true,
                owned_tool_candidates_push: true,
            }
        ));
    }
}

/// Exact V1 command tree: init, plan, and generate.
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
        /// Preview only approved generator-owned tool candidate infrastructure.
        /// Requires canonical generator repository identity and source approvals.
        #[arg(long, requires = "output_dir")]
        owned_tool_candidates_only: bool,
        /// Qualify only on the exact reviewed infrastructure branch push.
        #[arg(long, requires = "owned_tool_candidates_only")]
        owned_tool_candidates_push: bool,
    },
}
