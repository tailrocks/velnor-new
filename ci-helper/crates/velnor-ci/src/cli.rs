// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use velnor_model::{InvalidId, RepoPath};

mod gate;
mod report;
mod write_request;
pub use gate::GateArgs;
pub use report::ReportArgs;
pub use write_request::WriteRequestArgs;

#[derive(Debug, Parser)]
#[command(
    name = "velnor-ci",
    about = "Measures CI runs and compares the measurements"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(
        about = "Write ci-metrics.json and the lane summary; --check checks lane artifacts for cache waste"
    )]
    Report(ReportArgs),
    #[command(subcommand, about = "Collect and compare run metrics")]
    Bench(BenchCommand),
    #[command(
        subcommand,
        about = "Keep tracked files' modification times with a cached target directory, and audit the files packages read at compile time"
    )]
    Inputs(InputsCommand),
    #[command(
        about = "Print and write to $GITHUB_OUTPUT ready=true when the commit is on the default branch and all required checks outside the explicit --pr-only-check list passed in push jobs on that branch at the exact commit; ready=false while a check is unfinished; exit 1 on a failed check, invalid exemption or off-branch commit. Reads GITHUB_TOKEN, which needs contents: read, actions: read and statuses: read; PR-only exemptions also need checks: read and visible ruleset bypass_actors metadata"
    )]
    ReleaseGate(ReleaseGateArgs),
    #[command(
        about = "The required check: read the needs context from NEEDS_JSON, and the spec in the variable --spec-env names, and the plan outputs of the plan job the spec names, then exit 1 unless every job the plan scheduled succeeded, no job it did not schedule failed, and every package that listed tests in a selected test job ran at least one. Fails when the plan job failed, a plan output is missing or invalid, a job is missing from needs, or a result is cancelled or unknown. Prints a table and appends it to $GITHUB_STEP_SUMMARY"
    )]
    Gate(GateArgs),
    #[command(
        about = "Exit 1 unless the tag is rust-ci-v or tools-v followed by the version of this program"
    )]
    CheckTag(CheckTagArgs),
    #[command(
        subcommand,
        about = "Record what release-plz released, and publish a release's draft"
    )]
    Release(ReleaseCommand),
    #[command(
        about = "Select the Rust packages the changed files affect, and write rust_<root>_mode, rust_<root>_packages, rust_<root>_test_filter, rust_<root>_package_args (the package selection for cargo doc: --workspace for all, -p <name> for each selected package, empty for none), rust_<root>_doctests and rust_<root>_docs (run or skip; both run when GITHUB_EVENT_NAME is schedule) and rust_<root>_doctests_known (true when a fenced doctest was found, false otherwise) for each Cargo root to $GITHUB_OUTPUT; run in the checkout. Reads .velnor/rust-metadata.json instead of running cargo and fails when a Cargo.toml changed after velnor-gen generate wrote it. Fails when default_branch in .velnor/rust-ci.toml differs from repository.default_branch of the event payload. On a pull_request event, and on a push or merge_group event when on_main is \"affected\", it lists the changed files with git diff between the parents of the merge commit that is checked out (fetch-depth: 2, HEAD must be GITHUB_SHA) and reads the base text of a changed Cargo.lock or root Cargo.toml from the first parent with git show; it needs no token and no network, and selects everything when git cannot tell. Any other event, and a push or merge_group event when on_main is \"everything\", selects everything. --changed replaces the git listing"
    )]
    Plan(PlanArgs),
    #[command(
        about = "Write the plan request file the planner reads, from the run's context: the event name and payload, the commit, the repository and, on a workflow_dispatch run, the qualification inputs and the runner's GITHUB_* values. Each context value is an argument that defaults to its GITHUB_* variable. Needs no VELNOR_REQUEST_FILE. The file is written once, under $RUNNER_TEMP, and an existing file is an error"
    )]
    WriteRequest(WriteRequestArgs),
    #[command(
        about = "Run after the tools are installed: exit 1 naming each tool that mise does not report at its pinned version, append RUSTUP_AUTO_INSTALL=0 and the CI build profile to $GITHUB_ENV, and write the step output compat to $GITHUB_OUTPUT; run in the checkout"
    )]
    Prepare,
    #[command(
        about = "Write the crates mirror under $RUNNER_TEMP and install a Cargo wrapper for every working directory and Cargo home"
    )]
    CratesMirror(CratesMirrorArgs),
    #[command(
        about = "Use the runner's shared MBX store: when VELNOR_MBX_SHARED_DIR names an absolute, existing, symlink-free, writable directory and the event is trusted (a push or schedule on the default branch), append MBX_CACHE_DIR and a pod-private MBX_SHIMS_DIR under $RUNNER_TEMP to $GITHUB_ENV. Every other event, and a directory that fails a check (with a warning), keeps MBX's per-job store"
    )]
    MbxStore,
    #[command(
        about = "On a pull request or any other untrusted event, when VELNOR_MBX_SEED_DIR names an absolute, existing, symlink-free directory owned by this user or root and not world-writable, copy its actions/{cas,executable-identities,task-manifests,action-results} files that are missing into the private store $RUNNER_TEMP/velnor/mbx, in parallel and without ever writing to the seed, then append MBX_CACHE_DIR for that store to $GITHUB_ENV and log the files, bytes and seconds. Without the variable, with an empty seed, on a trusted event, or when a check fails (with a warning), it changes nothing and MBX builds cold"
    )]
    MbxSeed,
    #[command(
        about = "Run after the tools are installed, in the directory of a Cargo root: exit 1 unless .velnor/rust-metadata.json is exactly what velnor-gen generate writes for that root, which reads cargo metadata --no-deps --offline"
    )]
    CheckMetadata,
    #[command(
        about = "Resolve every requested feature configuration afresh, then cargo check each distinct full package feature map once, offline and locked"
    )]
    CheckConfigurations(CheckConfigurationsArgs),
    #[command(
        about = "Run in the directory of a Cargo root after the feature test lane: exit 1 unless the lane filter selects every test that exists only with the features, as cargo-nextest lists them. Lists the workspace three times (default features, the features, and the features under the filter) with cargo-nextest, so the build is the lane's own; ignored tests are exempt, and a listing error fails the guard"
    )]
    LaneGuard(LaneGuardArgs),
    #[command(
        about = "Run after every cache and after velnor-ci inputs restore: every action that must follow the restores. Gives each tracked file that shares its modification time with another tracked file a distinct time, one microsecond below the time it had and ordered by path, so that cargo, which picks the newest file of a package by time alone, names the same file after every restore. When VELNOR_BASE_MAX_AGE_DAYS is a positive number, first removes every restored target directory whose target/.velnor/born date is more than that many days old, so the run builds cold. Writes the step output expired=true when it removed one (an unreadable born date counts as expired) and expired=false otherwise, so a later step can plan a full run"
    )]
    Restored(crate::cli_cache::RestoredArgs),
    #[command(subcommand, about = "Check job image candidates")]
    Image(ImageCommand),
    #[command(
        subcommand,
        about = "Decide whether the run saves the build directory, and delete the older entries of its family"
    )]
    Cache(CacheCommand),
    #[command(
        about = "Run a command, stream its output unchanged, append it to velnor-run.log in $RUNNER_TEMP, and exit with its exit code"
    )]
    Run(RunArgs),
    #[command(
        about = "Verify the source preview runtime SDK that .velnor/generator.lock at PUBLIC_SOURCE pins (run PUBLIC_RUN_ID attempt PUBLIC_RUN_ATTEMPT of the image workflow): the lock change was approved by the parent commit, the preview's runs, jobs, annotated tag, immutable release, digests and Sigstore attestation (gh attestation verify with the pinned gh, signer pinned to the publication workflow commit) all match. Then install the SDK under $RUNNER_TEMP/source-preview-runtime (linux, darwin, receipt.json, bundle.json, identity.json) and write linux_sha256 and darwin_sha256 to $GITHUB_OUTPUT, which must be empty. Reads GH_TOKEN, RUNNER_TEMP, GITHUB_OUTPUT; exits 1 on any failed check"
    )]
    VerifyRuntime(PreviewGhArgs),
    #[command(
        about = "Run the verified Linux SDK in $RUNNER_TEMP/source-preview-runtime: copy it into a sealed memory file, check its sha256 equals VERIFIED_LINUX_SHA256, and exec it as `image-maintenance verify-mac-attestation` with PRODUCER_RUN_ID, PRODUCER_RUN_ATTEMPT, PRODUCER_ARTIFACT_ID and PUBLISHER_ARTIFACT_ID and only an allowlist of the environment. Linux only; exits 1 when a check fails"
    )]
    VerifyMacProof,
    #[command(
        about = "Run one step of the source preview publication (draft, prepare or publish): check the admitted source metadata and payloads in the working directory against the run, the annotated tag and the GitHub release, and write the step outputs to $GITHUB_OUTPUT. Reads GH_TOKEN, the AUTHORITY_* variables, SOURCE_ARTIFACT_ID, LINUX_PAYLOAD_ARTIFACT_ID, DARWIN_PAYLOAD_ARTIFACT_ID and, for prepare and publish, SIGN, PAIR_ORIGIN, PAIR_ID, RECEIPT_SHA256 and BUNDLE_SHA256; exits 1 on any failed check"
    )]
    RunAuthority(RunAuthorityArgs),
    #[command(
        about = "Write .velnor/rust-ci-timings.json from the last successful runs of the generated workflow on the default branch: the median of at least three ordinary runs, one entry per Cargo root, counting only jobs that ran on the runner in .velnor/rust-ci.toml (default ubuntu-latest); writes nothing with fewer; reads GITHUB_TOKEN; run in the checkout"
    )]
    Timings(TimingsArgs),
}

#[derive(Debug, Args)]
pub struct PreviewGhArgs {
    #[arg(
        long,
        help = "Absolute path of the pinned gh; defaults to the PREVIEW_GH variable"
    )]
    pub gh: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct RunAuthorityArgs {
    #[arg(long, help = "draft, prepare or publish")]
    pub mode: String,
    #[arg(
        long,
        help = "Absolute path of the pinned gh; defaults to the PREVIEW_GH variable"
    )]
    pub gh: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct CheckConfigurationsArgs {
    #[arg(
        long,
        default_value = "Cargo.toml",
        help = "Cargo manifest to resolve and check"
    )]
    pub manifest_path: PathBuf,
    #[arg(
        long,
        help = "JSON array of named feature configurations; without a file, check the default configuration"
    )]
    pub configurations: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct LaneGuardArgs {
    #[arg(
        long,
        value_parser = clap::builder::NonEmptyStringValueParser::new(),
        help = "Comma-separated features the lane step passes to cargo"
    )]
    pub features: String,
    #[arg(
        long,
        value_parser = clap::builder::NonEmptyStringValueParser::new(),
        help = "The lane's own nextest filter expression"
    )]
    pub filter: String,
    #[arg(
        long,
        value_parser = clap::builder::NonEmptyStringValueParser::new(),
        help = "Nextest binary id the lane never lists; repeatable"
    )]
    pub except_binary: Vec<String>,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(
        long,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        default_value = "false",
        value_parser = clap::value_parser!(bool),
        value_name = "BOOL",
        help = "Exit 1 when the command succeeds but its output reports no doctest ran; --require-doctests=false turns the check off"
    )]
    pub require_doctests: bool,
    #[arg(
        last = true,
        required = true,
        num_args = 1..,
        value_name = "COMMAND",
        help = "The command and its arguments, after --"
    )]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
pub struct TimingsArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
}

#[derive(Debug, Args)]
pub struct PlanArgs {
    #[arg(
        long,
        num_args = 1..,
        value_parser = repo_path,
        help = "Changed files, relative to the repository root, instead of the files of the pull request"
    )]
    pub changed: Vec<RepoPath>,
    #[arg(
        long,
        help = "Print each selected package with its reason, and the trigger when a root selects everything, instead of the outputs"
    )]
    pub explain: bool,
    #[arg(
        long,
        help = "Select everything, as a scheduled run does, whatever the event and the changed files"
    )]
    pub full: bool,
    #[arg(
        long = "base-file",
        value_name = "PATH=FILE",
        value_parser = base_file,
        help = "The text a changed Cargo.lock, Cargo.toml or configuration file had before the change: the repository path, an equals sign, and a file holding the old text. Without it a changed Cargo.lock or root Cargo.toml selects everything, and a changed file that makes a check do less (.velnor/rust-ci.toml, a Cargo.toml, a cargo, mise or nextest configuration) counts as adding every such setting it holds, which the run reports as a warning. Repeat for each file"
    )]
    pub base_files: Vec<BaseFile>,
}

#[derive(Debug, Clone)]
pub struct BaseFile {
    pub path: RepoPath,
    pub file: PathBuf,
}

fn repo_path(text: &str) -> Result<RepoPath, InvalidId> {
    RepoPath::new(text)
}

fn base_file(text: &str) -> Result<BaseFile, String> {
    let (path, file) = text
        .split_once('=')
        .ok_or_else(|| "expected PATH=FILE".to_owned())?;
    Ok(BaseFile {
        path: RepoPath::new(path).map_err(|e| e.to_string())?,
        file: PathBuf::from(file),
    })
}

#[derive(Debug, Args)]
pub struct CheckTagArgs {
    #[arg(help = "The release tag")]
    pub tag: String,
}

pub use crate::cli_release::{
    CheckConfigArgs, FamilyTagArgs, PreflightArgs, PublishArgs, RecordArgs, ReleaseCommand,
    ReleaseGateArgs,
};

#[derive(Debug, Args)]
pub struct InputTimesArgs {
    #[arg(long, default_value = "target")]
    pub target_dir: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum InputsCommand {
    #[command(
        about = "Capture directory inputs and bind them to build outputs for inputs audit. Known Cargo compiler commands may run twice to establish inventories; tests precompile and execute once, and arbitrary commands execute once. Bootstrap uses --workspace or explicit -p package selections and refuses to invalidate unselected packages"
    )]
    Build(InputsBuildArgs),
    #[command(
        about = "Write target/.velnor/inputs.tsv: path, blob id of the bytes on disk, modification time in nanoseconds and mode of every regular tracked file in the worktree; paths a TSV field cannot hold are counted and left out"
    )]
    Save(InputTimesArgs),
    #[command(
        about = "Set the saved modification time on every tracked file whose path, bytes on disk and mode equal the saved ones; report changed, new, deleted, absent and left-out files"
    )]
    Restore(InputTimesArgs),
    #[command(
        about = "Run after velnor-ci inputs build -- cargo build --workspace --all-targets in every Cargo root: read cargo's dep-info and build script output, and exit 1 when .velnor/rust-inputs.lock does not list a file that a package read at compile time from outside its own directory, lists a line nothing reads, or the build is incomplete: every unit an all-targets build makes (each target, its test harness when it is tested, each build script run, and the library and build script run of each path dependency outside the workspaces wherever cargo built it) needs a current unit, identified by cargo's fingerprint, in each dev-profile directory (debug or <triple>/debug) that holds a test harness of a member, with proc-macro libraries in the host debug directory (targets that need features are skipped). A unit with a cargo fingerprint and no dep-info, and Cargo roots whose target directories hold one another, are refused. Only the features, target and profile of that build are seen. Files that tests read at run time, and files a build script reads without naming them with cargo:rerun-if-changed, are not seen here; declare them with [[inputs]] in .velnor/rust-ci.toml"
    )]
    Audit(AuditArgs),
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
}

pub use crate::cli_cache::{CacheCommand, DecideArgs, TrimArgs};
#[derive(Debug, Subcommand)]
pub enum BenchCommand {
    #[command(about = "Turn a workflow run into a metrics file; reads GITHUB_TOKEN")]
    Collect(CollectArgs),
    #[command(about = "Print a markdown table comparing two metrics files")]
    Compare(CompareArgs),
}

#[derive(Debug, Args)]
pub struct CollectArgs {
    #[arg(help = "Workflow run id")]
    pub run_id: u64,
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(short, long, help = "Write the metrics file here")]
    pub output: Option<PathBuf>,
    #[arg(long, help = "Accept a run whose run_attempt is greater than 1")]
    pub allow_rerun: bool,
}

#[derive(Debug, Args)]
pub struct CompareArgs {
    #[arg(help = "Metrics file of the first run")]
    pub a: PathBuf,
    #[arg(help = "Metrics file of the second run")]
    pub b: PathBuf,
    #[arg(
        long,
        help = "Compare runs of different workflows or events and label the output with the workload caveat; runs of different repositories are never compared"
    )]
    pub allow_different_populations: bool,
}

#[derive(Debug, Args)]
pub struct CratesMirrorArgs {
    #[arg(long)]
    pub registry: String,
    #[arg(long)]
    pub cainfo: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum ImageCommand {
    #[command(
        about = "Check a job image candidate: exit 1 unless the OCI archive is a bounded linux/amd64 image whose blobs, layer DiffIDs and graph match the candidate record, the record's archive hash and size match the archive bytes, the build inputs under --root hash to the recorded values, and the record's source commit, ref and workflow identity equal the current GITHUB_* environment and its Buildx version equals buildx_version in .velnor/runner.lock. Prints PASS on success"
    )]
    VerifyCandidate(VerifyCandidateArgs),
}

#[derive(Debug, Args)]
pub struct VerifyCandidateArgs {
    #[arg(
        long,
        default_value = ".",
        help = "Source checkout the record's inputs hash against"
    )]
    pub root: PathBuf,
    #[arg(long, help = "The candidate OCI archive")]
    pub archive: PathBuf,
    #[arg(long, help = "The candidate record JSON")]
    pub record: PathBuf,
}
