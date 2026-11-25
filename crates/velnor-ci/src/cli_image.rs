// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OpensslMode {
    None,
    Sysroot,
    Vendored,
}

#[derive(Debug, Args)]
pub struct ImageStageArgs {
    #[arg(
        long,
        default_value = "target",
        help = "The Cargo target directory the binaries were built into; a symbolic link is followed"
    )]
    pub target_dir: PathBuf,
    #[arg(
        long,
        default_value = "x86_64-unknown-linux-gnu",
        help = "The Rust target triple"
    )]
    pub target: String,
    #[arg(
        long,
        default_value = "release",
        help = "The profile directory under the target triple (release, or the name of a custom profile)"
    )]
    pub profile: String,
    #[arg(long, help = "The newest glibc the image may need, as 2.NN")]
    pub glibc: String,
    #[arg(
        long,
        value_enum,
        default_value = "none",
        help = "How the build handles OpenSSL: none and vendored refuse a binary that needs libssl or libcrypto, sysroot accepts only libssl.so.3 and libcrypto.so.3"
    )]
    pub openssl: OpensslMode,
    #[arg(
        long,
        default_value = "dist",
        help = "The directory that receives the binaries; it must not exist or must be empty"
    )]
    pub to: PathBuf,
    #[arg(
        long,
        help = "Stage into a new directory with an unpredictable name below this directory (0700), instead of --to, and write its path as dir=<path> to $GITHUB_OUTPUT; the files are read-only"
    )]
    pub private_under: Option<PathBuf>,
    #[arg(
        long,
        help = "The checkout: every directory of --to below it must be a real directory, not a symbolic link; the working directory by default"
    )]
    pub within: Option<PathBuf>,
    #[arg(
        long = "bin",
        required = true,
        help = "A binary to stage; repeat for each"
    )]
    pub bins: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GateOnly {
    Tip,
    Exemptions,
}

#[derive(Debug, Args)]
pub struct ImageGateArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(long, help = "Full SHA of the commit that may publish")]
    pub sha: String,
    #[arg(
        long,
        default_value_t = 2400,
        help = "How long to wait for the required checks to finish; 0 checks once"
    )]
    pub wait_seconds: u64,
    #[arg(
        long,
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(1..=300),
        help = "Seconds between two looks at the checks"
    )]
    pub interval_seconds: u64,
    #[arg(
        long,
        help = "Write ready=false and exit 0 when the commit stops being the tip of the default branch"
    )]
    pub require_tip: bool,
    #[arg(
        long,
        value_enum,
        conflicts_with_all = ["require_tip", "verify_exemptions"],
        help = "Do one thing without waiting: `tip` checks that the commit is still the tip of the default branch and writes ready=true or ready=false; `exemptions` verifies that the PR-only exemptions sit under a non-bypassable ruleset (needs a token that can read bypass_actors, Administration: write, and at least one --pr-only-check) and exits 1 or writes ready=true"
    )]
    pub only: Option<GateOnly>,
    #[arg(
        long,
        help = "Also verify the PR-only exemptions inside the wait, which needs the same token; without it the exemptions are trusted because an earlier job verified them"
    )]
    pub verify_exemptions: bool,
    #[arg(
        allow_hyphen_values = true,
        long = "pr-only-check",
        help = "A required check that runs only on pull requests; repeat for each"
    )]
    pub pr_only_checks: Vec<String>,
    #[arg(
        allow_hyphen_values = true,
        long = "ignore-job",
        help = "A job of the workflow that this run belongs to, which the wait does not count as a required check: the gate itself and the jobs that need it. The other jobs of this workflow count like any push job, so a required job of the workflow is waited for and its result is the evidence; repeat for each"
    )]
    pub ignore_jobs: Vec<String>,
    #[arg(
        allow_hyphen_values = true,
        long = "require-published",
        help = "A job of the push run of the commit on the default branch, such as image-<name>-publish, that must have succeeded before the gate is ready; the wait covers a job that is still running, and a cancelled, skipped or failed job refuses"
    )]
    pub require_published: Option<String>,
}

#[derive(Debug, Args)]
pub struct ImagePreflightArgs {
    #[arg(long, help = "The Docker Hub repository as namespace/name")]
    pub repository: String,
    #[arg(long, help = "The tag that this commit would push")]
    pub tag: String,
}

#[derive(Debug, Args)]
pub struct ImageSysrootArgs {
    #[arg(long, help = "The sysroot lock file")]
    pub lock: PathBuf,
    #[arg(
        long,
        help = "The directory to unpack into; it is replaced when it exists"
    )]
    pub out: PathBuf,
    #[arg(
        long,
        default_value = "/usr/bin/dpkg-deb",
        help = "The dpkg-deb program that unpacks a package"
    )]
    pub dpkg_deb: PathBuf,
}

#[derive(Debug, Args)]
pub struct ImageChangesArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(
        long,
        help = "The event name; only pull_request reads the changed files"
    )]
    pub event: String,
    #[arg(
        long,
        help = "The pull request number, required for a pull_request event"
    )]
    pub pull_request: Option<String>,
    #[arg(
        allow_hyphen_values = true,
        long = "match",
        env = "VELNOR_IMAGE_MATCHES",
        value_delimiter = '\n',
        value_name = "IMAGE=GLOB",
        required = true,
        help = "A path pattern that selects an image, written as the paths of an on.pull_request filter (`*` stays inside a path segment, `**` crosses them); repeat for each, or put one per line in $VELNOR_IMAGE_MATCHES"
    )]
    pub matches: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ImageAuthorizeArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(long, help = "The release tag that was pushed, without refs/tags/")]
    pub tag: String,
    #[arg(long, help = "The login that pushed the tag (github.actor)")]
    pub actor: String,
    #[arg(
        long = "release-actor",
        help = "A login that may release; repeat for each. Without a match, an active tag ruleset must forbid creating, moving and deleting the tag"
    )]
    pub release_actors: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ImagePromoteArgs {
    #[arg(
        long = "image",
        required = true,
        help = "A registry repository such as ghcr.io/owner/name; repeat for each"
    )]
    pub images: Vec<String>,
    #[arg(long, help = "The version X.Y.Z to tag")]
    pub version: String,
    #[arg(
        long,
        help = "The full commit SHA whose sha-<commit> image is promoted"
    )]
    pub commit: String,
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(
        long,
        help = "The release tag, without refs/tags/; its live ref must still name --commit right before anything is tagged"
    )]
    pub tag: String,
}

#[derive(Debug, Args)]
pub struct ImageAdvanceArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(long, help = "The full SHA of the commit that this run pushed")]
    pub commit: String,
    #[arg(long, help = "The digest that this run pushed, as sha256:<hex>")]
    pub digest: String,
    #[arg(long, help = "The default-branch tag to move")]
    pub branch: String,
    #[arg(
        long = "image",
        required = true,
        help = "A registry repository such as ghcr.io/owner/name; repeat for each"
    )]
    pub images: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ImageVerifyArgs {
    #[arg(long, help = "The directory that holds the downloaded staged binaries")]
    pub dir: PathBuf,
    #[arg(
        long,
        help = "<binary>=<sha256> pairs separated by commas, as the build job's stage step reported them; the directory must hold exactly these regular files with these hashes"
    )]
    pub sums: String,
}

#[derive(Debug, Args)]
pub struct ImageGhcrArgs {
    #[arg(long, help = "The package as ghcr.io/owner/name")]
    pub repository: String,
}
