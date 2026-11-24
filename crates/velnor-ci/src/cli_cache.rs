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
        about = "Read velnor-run.log in $RUNNER_TEMP and write the step output save=true when the run compiled, checked or documented at least one crate, save=false otherwise or when any checkout Cargo.lock has dependency sources not proven public (a lock whose only non-public sources are git dependencies with no credentials in their URL counts as public only when the environment variable VELNOR_REPOSITORY_PRIVATE is exactly true, the answer of the `cache private-repo` step that the workflow copies into the env of this step; this command makes no API request for it and reads no token, and an empty or unset value refuses such a lock, and then the sources output is false when the saved cargo directories hold a credential, a scan that VELNOR_SOURCES_RESTORED=true skips because an exact sources hit saves nothing); a missing or unreadable run log is an error; with true, remove <target>/<profile>/incremental and <target>/<triple>/<profile>/incremental, remove the --archive file, write target/.velnor/born with today's date when it is missing, and print the size of --target-dir"
    )]
    Decide(DecideArgs),
    #[command(
        about = "Write private=true to $GITHUB_OUTPUT only when GET /repos/$GITHUB_REPOSITORY on the API in VELNOR_API_URL, with the job token in GITHUB_TOKEN, answers for this repository with private=true and a visibility of private (an internal repository is not private); writes private=false and exits 0 for a public or internal repository, a failed lookup, a missing token or any other answer, so that no cache of private dependencies is restored or saved; a lookup that did not confirm the visibility (a failure, a missing token or an answer that does not name this repository or its visibility) also writes unavailable=true, which `report` carries into the metrics so that the Cache check fails"
    )]
    PrivateRepo,
    #[command(
        about = "Look up each commit-pinned github.com git dependency of the checkout Cargo.lock files once, with the job GITHUB_TOKEN (sent only to https://api.github.com, never redirected), and write the step output verdicts={\"owner/name\":true} for the lanes of this run; a failed or unproven lookup is recorded as false and does not fail the step, but an answer that says the repository is private (404 or private: true) fails it, naming the repository, within a 10-second budget"
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
        about = "List every GitHub Actions cache entry of the repository whose key starts with velnor-private-, on every ref, and delete them only with --yes (a dry run otherwise); prints counts and bytes, never a token. Run it once when a private repository becomes public, because a fork pull request of a public repository can restore a default-branch entry by its exact key; it does not look at the repository visibility, so it also removes the entries of a repository that is still private. It covers only velnor-private- keys: before a private repository goes public, delete ALL its caches (`gh cache delete --all -R <owner/repo>`), because helper 0.1.111 stored private sources and build outputs under velnor-src- and velnor-tgt- keys. With --yes it refuses, and deletes nothing, while any workflow of the repository is enabled (state active; disable every workflow first), when the workflow listing is incomplete, and while a snapshot of the workflow runs shows any that hasn't finished (in progress, queued, waiting, requested or pending); the snapshot is best-effort, since a run that starts or changes status inside the query window can still slip through and save a private entry, so keep every workflow disabled until the repository is public; a dry run only warns about unfinished runs. With --if-public it asks GET /repos/{repo} and keeps the entries only when the repository is private, deletes them for a public or internal repository and when the lookup fails, runs none of the workflow checks (the generated rust-trim job is itself a running workflow), tries every delete, and reports all failures together. Reads GITHUB_TOKEN, which needs actions: write to delete, and GITHUB_REPOSITORY"
    )]
    PurgePrivate(PurgePrivateArgs),
}

#[derive(Debug, Args)]
pub struct PurgePrivateArgs {
    #[arg(long, help = "Delete the entries; without it only report them")]
    pub yes: bool,
    #[arg(
        long,
        conflicts_with = "yes",
        help = "Delete the entries unless GET /repos/{repo} proves the repository private; a failed lookup deletes them; skips the workflow checks of --yes; tries every delete and reports the failures together"
    )]
    pub if_public: bool,
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
    #[arg(long, hide = true, default_value_t = crate::public_probe::EARLY_PROBE_SECONDS)]
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
