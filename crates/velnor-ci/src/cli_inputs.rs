// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum InputsCommand {
    #[command(
        about = "Capture directory inputs and bind them to build outputs for inputs audit. Known Cargo compiler commands may run twice to establish inventories; tests precompile and execute once, and arbitrary commands execute once. Bootstrap uses --workspace or explicit -p package selections and refuses to invalidate unselected packages"
    )]
    Build(InputsBuildArgs),
    #[command(
        about = "Write a root-specific snapshot under .velnor in the target directory: inputs.tsv at the Git root, inputs-<SHA256 of Git-relative root prefix>.tsv below it. Record path, blob id of the bytes on disk, modification time in nanoseconds and mode of every regular tracked file in scope; paths a TSV field cannot hold are counted and left out"
    )]
    Save(InputsSaveArgs),
    #[command(
        about = "Set the saved modification time on every tracked file whose path, bytes on disk and mode equal the saved ones; report changed, new, deleted, absent and left-out files"
    )]
    Restore(InputsArgs),
    #[command(about = "Restore all root and target directory pairs with one checkout analysis")]
    RestoreAll(InputsBatchArgs),
    #[command(
        about = "Run after velnor-ci inputs build -- cargo build --workspace --all-targets in every Cargo root: read cargo's dep-info and build script output, and exit 1 when .velnor/rust-inputs.lock does not list a file that a package read at compile time from outside its own directory, lists a line nothing reads, or the build is incomplete: every unit an all-targets build makes (each target, its test harness when it is tested, each build script run, and the library and build script run of each path dependency outside the workspaces wherever cargo built it) needs a current unit, identified by cargo's fingerprint, in each dev-profile directory (debug or <triple>/debug) that holds a test harness of a member, with proc-macro libraries in the host debug directory (targets that need features are skipped). A unit with a cargo fingerprint and no dep-info, and Cargo roots whose target directories hold one another, are refused. Only the features, target and profile of that build are seen. Files that tests read at run time, and files a build script reads without naming them with cargo:rerun-if-changed, are not seen here; declare them with [[inputs]] in .velnor/rust-ci.toml"
    )]
    Audit(AuditArgs),
}

#[derive(Debug, Args)]
pub struct InputsArgs {
    #[arg(
        long,
        default_value = "target",
        help = "Cargo target directory containing root-specific .velnor/inputs*.tsv snapshots, relative to the working directory or absolute"
    )]
    pub target_dir: PathBuf,
}

#[derive(Debug, Args)]
pub struct InputsSaveArgs {
    #[command(flatten)]
    pub input: InputsArgs,
    #[arg(long)]
    pub scope_cache: Option<PathBuf>,
    #[arg(
        long,
        conflicts_with = "target_dir",
        help = "Environment variable containing a hex-encoded UTF-8 target path"
    )]
    pub target_env: Option<String>,
}

#[derive(Debug, Args)]
pub struct InputsBatchArgs {
    #[arg(long)]
    pub scope_cache: Option<PathBuf>,
    #[arg(long, num_args = 2, required_unless_present = "roots_env", conflicts_with = "roots_env", value_names = ["ROOT", "TARGET"])]
    pub root_target: Vec<PathBuf>,
    #[arg(
        long,
        help = "Environment variable containing JSON pairs of hex-encoded UTF-8 root and target paths"
    )]
    pub roots_env: Option<String>,
}

#[derive(Debug, Args)]
pub struct RestoredArgs {
    #[arg(long, conflicts_with = "targets_env")]
    pub target_dir: Vec<PathBuf>,
    #[arg(
        long,
        help = "Environment variable containing a JSON array of hex-encoded UTF-8 target paths"
    )]
    pub targets_env: Option<String>,
}

#[derive(Debug, Args)]
pub struct InputsBuildArgs {
    /// Additional source directory outside the repository to inventory before the build; repeat for each directory
    #[arg(long)]
    pub directory: Vec<PathBuf>,
    #[arg(last = true, required = true, num_args = 1.., value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
pub struct AuditArgs {
    #[arg(
        long,
        help = "Write .velnor/rust-inputs.lock, one path<TAB>package line per read, instead of checking it"
    )]
    pub write: bool,
    #[arg(
        long,
        value_name = "DIR",
        conflicts_with = "write",
        help = "Audit only the Cargo root whose manifest is in DIR (. for the repository root), plus every root of its workspace group, so each lane that built a root audits it. Other roots are neither read nor required to be built, and only the lock lines of the audited packages are compared"
    )]
    pub root: Option<String>,
}
