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
        #[arg(long = "output-dir", value_name = "PATH")]
        output_dir: Option<PathBuf>,
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, Command};

    /// Parse `argv` (including the binary name) or return the Clap message.
    fn parse(argv: &[&str]) -> Result<Cli, String> {
        Cli::try_parse_from(argv).map_err(|error| error.to_string())
    }

    #[test]
    fn parses_init() -> Result<(), String> {
        let cli = parse(&["velnor-actions", "init"])?;
        assert!(matches!(cli.command, Command::Init));
        Ok(())
    }

    #[test]
    fn parses_plan() -> Result<(), String> {
        let cli = parse(&["velnor-actions", "plan"])?;
        assert!(matches!(cli.command, Command::Plan));
        Ok(())
    }

    #[test]
    fn parses_generate_without_output_dir() -> Result<(), String> {
        let cli = parse(&["velnor-actions", "generate"])?;
        assert!(matches!(
            cli.command,
            Command::Generate { output_dir: None }
        ));
        Ok(())
    }

    #[test]
    fn parses_generate_with_output_dir() -> Result<(), String> {
        let cli = parse(&["velnor-actions", "generate", "--output-dir", "/tmp/preview"])?;
        let Command::Generate { output_dir } = cli.command else {
            return Err("expected generate command".to_owned());
        };
        assert_eq!(output_dir, Some(PathBuf::from("/tmp/preview")));
        Ok(())
    }

    #[test]
    fn rejects_bare_invocation() {
        let error = Cli::try_parse_from(["velnor-actions"]);
        assert!(error.is_err());
    }

    #[test]
    fn rejects_unknown_command() {
        assert!(Cli::try_parse_from(["velnor-actions", "__internal"]).is_err());
        assert!(Cli::try_parse_from(["velnor-actions", "bogus"]).is_err());
    }

    #[test]
    fn rejects_unknown_flags() {
        assert!(Cli::try_parse_from(["velnor-actions", "--root", "."]).is_err());
        assert!(Cli::try_parse_from(["velnor-actions", "plan", "--format", "json"]).is_err());
        assert!(Cli::try_parse_from(["velnor-actions", "init", "--force"]).is_err());
    }

    #[test]
    fn rejects_extra_positionals_and_missing_values() {
        assert!(Cli::try_parse_from(["velnor-actions", "init", "extra"]).is_err());
        assert!(Cli::try_parse_from(["velnor-actions", "plan", "extra"]).is_err());
        assert!(Cli::try_parse_from(["velnor-actions", "generate", "--output-dir"]).is_err());
    }

    #[test]
    fn usage_errors_exit_with_code_two() {
        for argv in [
            vec!["velnor-actions"],
            vec!["velnor-actions", "bogus"],
            vec!["velnor-actions", "--root", "."],
        ] {
            let code = Cli::try_parse_from(argv)
                .err()
                .map(|error| error.exit_code());
            assert_eq!(code, Some(2));
        }
    }
}
