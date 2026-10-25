//! Clap command tree for `velnor-actions`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[path = "args_image_maintenance.rs"]
mod image_maintenance;
pub(crate) use image_maintenance::ImageMaintenanceCommand;

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
    /// List `.github` entries that generation would not emit or retire.
    ///
    /// Prints one repository-relative path per line to stdout, honoring
    /// `[github] allow` and `[workflow] preserve_workflows`. Exits 1 when
    /// `[github] unmanaged = "deny"` and any entry is listed.
    Unmanaged,
    /// Read or rewrite `.velnor/config.toml`.
    Config {
        /// Config subcommand.
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Pin private cross-repository dependency sources.
    Deps {
        /// Deps subcommand.
        #[command(subcommand)]
        command: DepsCommand,
    },
    /// Pin the Playwright browsers Bun workspace members need.
    Playwright {
        /// Playwright subcommand.
        #[command(subcommand)]
        command: PlaywrightCommand,
    },
    /// Author a data-only Homebrew adoption overlay from protected evidence.
    Homebrew {
        /// Homebrew adoption authoring operation.
        #[command(subcommand)]
        command: HomebrewCommand,
    },
    /// Maintain exact-source native image candidates.
    ImageMaintenance {
        #[command(subcommand)]
        command: ImageMaintenanceCommand,
    },
    /// Author qualified-tool candidates offline from independently pinned local archives.
    ///
    /// Supply archive SHA-256 values independently. Runs fixed version probes on
    /// the native process platform, without fetching or compiling tools. Output
    /// is authoring evidence; runtime verification and configuration stay unchanged.
    QualifyTools {
        /// Strict versioned request JSON; absolute path.
        #[arg(long, value_name = "ABS")]
        request: PathBuf,
        /// Fresh owned staging directory; must not exist.
        #[arg(long, value_name = "FRESHABS")]
        work_dir: PathBuf,
        /// New authoring candidate JSON; must not exist.
        #[arg(long, value_name = "NEWABS")]
        output: PathBuf,
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

/// Private dependency subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum DepsCommand {
    /// Resolve every `[private_sources]` declaration to a commit and SHA-256 and
    /// write `.velnor/deps.lock`. Uses your own `GH_TOKEN` or `GITHUB_TOKEN`.
    Lock,
}

/// Playwright browser subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum PlaywrightCommand {
    /// Resolve the browsers each Bun workspace member declares to
    /// SHA-256-pinned archives and write `.velnor/playwright.lock`. Downloads
    /// the `playwright-core` version `bun.lock` pins (verified against its
    /// integrity) and every browser archive; the digests are trust-on-first-use.
    Lock,
}

/// Protected Homebrew adoption authoring operations.
#[derive(Debug, Subcommand)]
pub(crate) enum HomebrewCommand {
    /// Export or check the canonical Homebrew preview source policy.
    SourcePolicy {
        /// Project checkout whose configuration and inventory define policy.
        #[arg(long, value_name = "CHECKOUT")]
        root: PathBuf,
        /// New absolute file path for the canonical policy export.
        #[arg(
            long,
            value_name = "EXCLUSIVE_NEW",
            conflicts_with = "check",
            required_unless_present = "check"
        )]
        output: Option<PathBuf>,
        /// Check `.velnor/homebrew-release-policy.json` in the checkout.
        #[arg(long, conflicts_with = "output")]
        check: bool,
    },
    /// Produce the exact initial seed overlay from the published SDK release.
    AdoptionSeed {
        /// Protected transition identifier.
        #[arg(long, value_name = "ID")]
        transition: String,
        /// Local copy of the published SDK release manifest.
        #[arg(long = "sdk-manifest", value_name = "ABS")]
        sdk_manifest: PathBuf,
        /// Fresh external output directory.
        #[arg(long = "output-dir", value_name = "FRESHABS")]
        output_dir: PathBuf,
    },
    /// Produce the normal structural cutover overlay over the protected seed.
    AdoptionCutover {
        /// Protected transition identifier.
        #[arg(long, value_name = "ID")]
        transition: String,
        /// Local copy of the published SDK release manifest.
        #[arg(long = "sdk-manifest", value_name = "ABS")]
        sdk_manifest: PathBuf,
        /// Fresh external output directory.
        #[arg(long = "output-dir", value_name = "FRESHABS")]
        output_dir: PathBuf,
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
