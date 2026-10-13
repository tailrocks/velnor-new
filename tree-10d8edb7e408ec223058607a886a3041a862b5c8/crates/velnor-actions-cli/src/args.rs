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
        /// Preview only the fixed published Foundation native qualification workflow.
        /// Requires canonical generator identity and an empty external destination.
        #[arg(long, requires = "output_dir")]
        foundation_qualification_only: bool,
    },
}

#[cfg(test)]
mod foundation_tests {
    use super::{Cli, Command};
    use clap::{CommandFactory, Parser, error::ErrorKind};

    #[test]
    fn foundation_preview_requires_external_destination() {
        let error = Cli::try_parse_from([
            "velnor-actions",
            "generate",
            "--foundation-qualification-only",
        ])
        .expect_err("explicit output required");
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        let parsed = Cli::try_parse_from([
            "velnor-actions",
            "generate",
            "--foundation-qualification-only",
            "--output-dir",
            "/tmp/foundation-source-preview",
        ])
        .expect("closed Foundation source preview");
        assert!(matches!(
            parsed.command,
            Command::Generate {
                output_dir: Some(_),
                foundation_qualification_only: true,
            }
        ));
    }

    #[test]
    fn foundation_help_exposes_only_fixed_source_mode() {
        let mut cli = Cli::command();
        let help = cli
            .find_subcommand_mut("generate")
            .expect("generate command")
            .render_long_help()
            .to_string();
        assert!(help.contains("--foundation-qualification-only"));
        assert!(help.contains("--output-dir <PATH>"));
        assert!(!help.contains("--foundation-action-ref"));
        let error = Cli::try_parse_from([
            "velnor-actions",
            "generate",
            "--foundation-qualification-only",
            "--output-dir",
            "/tmp/foundation-source-preview",
            "--foundation-action-ref",
            "caller/repository/action@main",
        ])
        .expect_err("caller source reference unavailable");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }
}
