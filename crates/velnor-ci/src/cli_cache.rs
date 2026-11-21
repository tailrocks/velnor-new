// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    #[command(
        about = "In trusted workflow_run cleanup, validate the completed same-repository PR run and keep its newest delta per lane"
    )]
    TrimDeltas(crate::delta_cli::TrimDeltasArgs),
    #[command(
        about = "Read velnor-run.log in $RUNNER_TEMP and write the step output save=true when the run compiled, checked or documented at least one crate, save=false otherwise or when any checkout Cargo.lock has dependency sources not proven public (a lock whose only non-public sources are git dependencies with no credentials in their URL counts as public only when the environment variable VELNOR_REPOSITORY_PRIVATE is exactly true, the answer of the `cache private-repo` step that the workflow copies into the env of this step; this command makes no API request for it and reads no token, and an empty or unset value refuses such a lock, and then the sources output is false when the saved cargo directories hold a credential); a missing or unreadable run log is an error; with true, remove <target>/<profile>/incremental and <target>/<triple>/<profile>/incremental, remove the --archive file, write target/.velnor/born with today's date when it is missing, and print the size of --target-dir"
    )]
    Decide(DecideArgs),
    #[command(
        about = "Write private=true to $GITHUB_OUTPUT only when GET /repos/$GITHUB_REPOSITORY on the API in VELNOR_API_URL, with the job token in GITHUB_TOKEN, answers for this repository with private=true and a visibility of private (an internal repository is not private); writes private=false and exits 0 for a public or internal repository, a failed lookup, a missing token or any other answer, so that no cache of private dependencies is restored or saved"
    )]
    PrivateRepo,
    #[command(
        about = "Look up each commit-pinned github.com git dependency of the checkout Cargo.lock files once, with the job GITHUB_TOKEN (sent only to https://api.github.com, never redirected), and write the step output verdicts={\"owner/name\":true} for the lanes of this run; a failed lookup is recorded as false and never fails the step"
    )]
    PublicSources(PublicSourcesArgs),
    #[command(
        about = "Delete all but the newest entries whose key starts with --prefix on $GITHUB_REF, through the GitHub API; reads GITHUB_TOKEN, which needs actions: write, and GITHUB_REPOSITORY. A missing token or a failed API call exits with an error"
    )]
    Trim(TrimArgs),
    #[command(about = "Require an exact saved cache entry through the read-only cache API")]
    Verify(VerifyArgs),
    #[command(about = "Fail the Cache check when a lane could not verify its uploads")]
    Check(CheckArgs),
    #[command(about = "Enforce the repository cache budget while protecting newest bases")]
    Budget(BudgetArgs),
    #[command(
        about = "List every GitHub Actions cache entry of the repository whose key starts with velnor-private-, on every ref, and delete them only with --yes (a dry run otherwise); prints counts and bytes, never a token. Run it once when a private repository becomes public, because a fork pull request of a public repository can restore a default-branch entry by its exact key; it does not look at the repository visibility, so it also removes the entries of a repository that is still private. It covers only velnor-private- keys: before a private repository goes public, delete ALL its caches (`gh cache delete --all -R <owner/repo>`), because helper 0.1.111 stored private sources and build outputs under velnor-src- and velnor-tgt- keys. With --yes it refuses, and deletes nothing, while the repository has any workflow run that hasn't finished (in progress, queued, waiting, requested, pending or awaiting approval; wait for them or disable workflows, then run it again); a dry run only warns. Reads GITHUB_TOKEN, which needs actions: write to delete, and GITHUB_REPOSITORY"
    )]
    PurgePrivate(PurgePrivateArgs),
    #[command(
        about = "Delete the one cache entry with --key and --version (the version is printed by the tools-verify notice), through the GitHub API; refuses when no entry or more than one entry matches; reads GITHUB_TOKEN, which needs actions: write"
    )]
    DeleteEntry(DeleteEntryArgs),
}

#[derive(Debug, Args)]
pub struct DeleteEntryArgs {
    #[arg(long, help = "The exact cache key")]
    pub key: String,
    #[arg(long, help = "The cache entry version, as the cache API lists it")]
    pub version: String,
    #[arg(
        long = "ref",
        help = "The ref the entry belongs to, such as refs/heads/main; without it every ref is searched"
    )]
    pub reference: Option<String>,
    #[arg(long, help = "Repository as owner/name, default GITHUB_REPOSITORY")]
    pub repo: Option<String>,
}

#[derive(Debug, Args)]
pub struct PurgePrivateArgs {
    #[arg(long, help = "Delete the entries; without it only report them")]
    pub yes: bool,
    #[arg(long, help = "Repository as owner/name, default GITHUB_REPOSITORY")]
    pub repo: Option<String>,
}

#[derive(Debug, Args)]
pub struct BudgetArgs {
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, help = "Repository as owner/name, default GITHUB_REPOSITORY")]
    pub repo: Option<String>,
    #[arg(long, default_value_t = velnor_rust::DEFAULT_CACHE_BUDGET_BYTES, value_parser = clap::value_parser!(u64).range(0..=velnor_rust::DEFAULT_CACHE_BUDGET_BYTES))]
    pub budget_bytes: u64,
}

#[derive(Debug, Args)]
pub struct DecideArgs {
    #[arg(long, hide = true, default_value = "https://api.github.com")]
    pub public_probe_api: String,
    #[arg(long, hide = true, default_value_t = crate::public_probe::DEFAULT_TOTAL_SECONDS)]
    pub public_probe_seconds: u64,
    #[arg(
        long,
        default_value = "Cargo.toml",
        help = "Detected Cargo root manifests contributing to this job"
    )]
    pub manifest_path: Vec<PathBuf>,
    #[arg(
        long,
        required_unless_present = "paths_env",
        conflicts_with = "paths_env",
        help = "The build directory the cache saves"
    )]
    pub target_dir: Option<PathBuf>,
    #[arg(
        long,
        conflicts_with = "paths_env",
        help = "A nextest archive file to remove before the save"
    )]
    pub archive: Option<PathBuf>,
    #[arg(
        long,
        help = "Environment variable containing JSON [hex target, optional hex archive] paths"
    )]
    pub paths_env: Option<String>,
}

#[derive(Debug, Args)]
pub struct PublicSourcesArgs {
    #[arg(long, hide = true, default_value = "https://api.github.com")]
    pub public_probe_api: String,
    #[arg(long, hide = true, default_value_t = crate::public_probe::DEFAULT_TOTAL_SECONDS)]
    pub public_probe_seconds: u64,
    #[arg(
        long,
        default_value = "Cargo.toml",
        help = "Detected Cargo root manifests of the checkout"
    )]
    pub manifest_path: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub struct TrimArgs {
    #[arg(long, help = "Key prefix of the family to trim")]
    pub prefix: String,
    #[arg(long, help = "Entries to keep, default 2")]
    pub keep: Option<usize>,
}

#[derive(Debug, Args)]
pub struct VerifyArgs {
    #[arg(long)]
    pub key: String,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[arg(long)]
    pub success: String,
}
