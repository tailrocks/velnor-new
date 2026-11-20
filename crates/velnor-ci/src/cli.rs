// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

mod fetch_reports;
pub use crate::cli_inputs::{
    AuditArgs, InputsArgs, InputsBatchArgs, InputsBuildArgs, InputsCommand, InputsSaveArgs,
    RestoredArgs,
};
mod gate;
mod report;
mod resolve_qualification;
pub use fetch_reports::FetchReportsArgs;
mod write_request;
pub use crate::cli_args::{
    BaseFile, BenchCommand, BudgetArgs, CheckConfigurationsArgs, CollectArgs, CompareArgs,
    CratesMirrorArgs, ImageCommand, MisePinArgs, PlanArgs, PlanMode, PrepareArgs,
    QualificationCommand, QualificationCompareArgs, ToolsLocalArgs, VerifyCandidateArgs,
};
pub use crate::cli_cache::{CacheCommand, DecideArgs, TrimArgs};
pub use gate::GateArgs;
pub use report::ReportArgs;
pub use resolve_qualification::ResolveQualificationArgs;
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
        subcommand,
        about = "Restore and stage the PR's cumulative build delta"
    )]
    Delta(crate::delta_cli::DeltaCommand),
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
        about = "Download every report artifact the plan expects, all at once from one listing of the run: read the artifact names from matrix.include[].artifact_id of <run-dir>/plan.json, list the run's artifacts, download each expected one over --connections connections and extract it to <run-dir>/reports/<artifact name>; the listing and every download get three attempts, all artifacts together may extract to at most --max-extract-bytes, and two live artifacts with one expected name are an error. Every expected artifact is attempted before the command fails; exit 1 names each one that was never uploaded, expired or could not be downloaded, and a missing, oversized, non-regular, symbolic-link or malformed plan.json (a repeated object key is malformed). Reads GITHUB_TOKEN, which needs actions: read"
    )]
    FetchReports(FetchReportsArgs),
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
        about = "On a push, write the step output skip=true to $GITHUB_OUTPUT when a merge_group run of this same workflow (GITHUB_WORKFLOW_REF) already succeeded on GITHUB_SHA, and skip=false otherwise. Fails safe: any other event, missing variable or API error writes skip=false and exits 0. Reads GITHUB_TOKEN, which needs actions: read"
    )]
    MergeGroupPassed,
    #[command(
        about = "On a pull_request run, print a ::warning:: when no active ruleset of the base branch (GITHUB_BASE_REF) has a merge_queue rule, because pr_ci = \"label\" is safe only with a merge queue (one GET /repos/$GITHUB_REPOSITORY/rules/branches/<base>, five pages of 100 rules at most). A failed lookup (403, 404, a bad response) prints a warning that says so. Prints nothing when a queue is required or on any other event, and always exits 0. Reads GITHUB_TOKEN, which needs only the default read access"
    )]
    MergeQueueCheck,
    #[command(
        about = "Write the plan request file the planner reads, from the run's context: the event name and payload, the commit, the repository and, on a workflow_dispatch run, the qualification inputs and the runner's GITHUB_* values. Each context value is an argument that defaults to its GITHUB_* variable. Needs no VELNOR_REQUEST_FILE. The file is written once, under $RUNNER_TEMP, and an existing file is an error"
    )]
    WriteRequest(WriteRequestArgs),
    #[command(
        about = "On a workflow_dispatch qualification run, resolve the predecessor the request names: re-read that run attempt through the GitHub API (default branch, event, conclusion, workflow path, repository), walk its receipt artifacts, derive the source delta from git for useful_delta, and write $RUNNER_TEMP/velnor/qualification-cache-admission.json. Writes nothing for a phase with no predecessor. Reads GH_TOKEN (or GITHUB_TOKEN) and GITHUB_API_URL; run in the checkout"
    )]
    ResolveQualification(ResolveQualificationArgs),
    #[command(
        about = "Run after the tools are installed, in the checkout: exit 1 naming each tool that mise does not report at its pinned version, append RUSTUP_AUTO_INSTALL=0 and the CI build profile to $GITHUB_ENV, and write the step output compat and one step output lock_<root> per Cargo root to $GITHUB_OUTPUT (--lock-root and --no-locks narrow the roots). lock_<root> is the SHA-256 of the root's Cargo.lock without the versions of the workspace's own path packages, which the Target cache key uses. Reads .velnor/rust-metadata.json, which velnor-gen generate writes, and fails when it is missing or unreadable"
    )]
    Prepare(PrepareArgs),
    #[command(
        about = "Write the crates mirror under $RUNNER_TEMP and install a Cargo wrapper for every working directory and Cargo home"
    )]
    CratesMirror(CratesMirrorArgs),
    #[command(
        about = "Run cargo fetch --locked for each manifest with the token in VELNOR_PRIVATE_TOKEN given to git for that command only, then revoke the token with DELETE /installation/token on the API in VELNOR_API_URL. Starts cargo and git by absolute path from the tool directories the runner installed, with an environment it builds itself, so that the checkout's configuration cannot redirect the token. Without a token it runs cargo fetch --locked --offline and fails when the restored caches lack a dependency"
    )]
    Fetch(FetchArgs),
    #[command(
        about = "Run before the private-source token step on a merge_group event: read the queue entry from VELNOR_MERGE_GROUP_HEAD_REF (gh-readonly-queue/<base>/pr-<number>-<sha>), look the pull request up with GET /repos/$GITHUB_REPOSITORY/pulls/<number> on the API in VELNOR_API_URL with the job token in GITHUB_TOKEN, and write allowed=true to $GITHUB_OUTPUT only when its head repository is $GITHUB_REPOSITORY, its author's login does not end in [bot] and its head sha is the queue entry's sha, and when the live queue ref of that entry still points at $GITHUB_SHA, the commit this run checked out. Every other queue entry of the same base whose commit is in that checkout (one GET /repos/$GITHUB_REPOSITORY/compare/<base>...$GITHUB_SHA, at most three pages of 100 commits) must pass the same pull request checks; a queue of 100 entries or more fails. Writes allowed=false and exits 0 for a malformed reference, a failed lookup, a fork or bot entry, a sha mismatch, a queue that was rebuilt, a bound that is exceeded or anything else, so that no token is created"
    )]
    PrivateTokenAllowed,
    #[command(
        about = "Use the runner's shared MBX store: when VELNOR_MBX_SHARED_DIR names an absolute, existing, symlink-free, writable directory and the event is trusted (a push or schedule on the default branch), append MBX_CACHE_DIR and a pod-private MBX_SHIMS_DIR under $RUNNER_TEMP to $GITHUB_ENV. Every other event, and a directory that fails a check (with a warning), keeps MBX's per-job store"
    )]
    MbxStore,
    #[command(
        about = "On a pull request or any other untrusted event, when VELNOR_MBX_SEED_DIR names an absolute, existing, symlink-free directory owned by this user or root and not world-writable, copy its actions/{cas,executable-identities,task-manifests,action-results} files that are missing into the private store $RUNNER_TEMP/velnor/mbx, in parallel and without ever writing to the seed, then append MBX_CACHE_DIR for that store to $GITHUB_ENV and log the files, bytes and seconds. Without the variable, with an empty seed, on a trusted event, or when a check fails (with a warning), it changes nothing and MBX builds cold"
    )]
    MbxSeed,
    #[command(
        about = "Keep the mise and rustup tool directories on the node's own disk. restore copies ~/.local/share/mise and ~/.rustup from tools/<key> of the store (VELNOR_MBX_SHARED_DIR on a trusted event, VELNOR_MBX_SEED_DIR otherwise) and writes hit=true or hit=false to $GITHUB_OUTPUT, so the network cache action can be skipped on a hit; save, on a trusted event only, copies them into tools/<key> with an atomic rename, keeping the two newest keys. A missing store, a failed check or a failed copy is a miss, never an error"
    )]
    ToolsLocal(ToolsLocalArgs),
    #[command(
        about = "Print the exact release that the workspace's mise.toml (and .mise.toml) pins for <tool>, with a leading v removed, and fail unless it is one X.Y.Z release that mise.lock, when present, resolves to the same version. A range, latest, a loose or leading-zero version, a missing pin or two different pins fail. Appends version=<release> to $GITHUB_OUTPUT when that variable is set"
    )]
    MisePin(MisePinArgs),
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
        subcommand,
        about = "Check the [[test_groups]] of .velnor/rust-ci.toml against the tests cargo-nextest lists"
    )]
    TestGroups(TestGroupsCommand),
    #[command(
        about = "Run after every cache and after velnor-ci inputs restore: every action that must follow the restores. Gives each tracked file that shares its modification time with another tracked file a distinct time, one microsecond below the time it had and ordered by path, so that cargo, which picks the newest file of a package by time alone, names the same file after every restore. When VELNOR_BASE_MAX_AGE_DAYS is a positive number, first removes every restored target directory whose target/.velnor/born date is more than that many days old, so the run builds cold. Writes the step output expired=true when it removed one (an unreadable born date counts as expired) and expired=false otherwise, so a later step can plan a full run"
    )]
    Restored(RestoredArgs),
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
    #[command(subcommand, about = "Work on the shared Sources cache")]
    Sources(SourcesCommand),
    #[command(
        about = "Write .velnor/rust-ci-timings.json from the last successful runs of the generated workflow on the default branch: the median of at least three ordinary runs, one entry per Cargo root, counting only jobs that ran on the runner in .velnor/rust-ci.toml (default ubuntu-latest); writes nothing with fewer; reads GITHUB_TOKEN; run in the checkout"
    )]
    Timings(TimingsArgs),
    #[command(
        about = "Exit 1 unless VELNOR_EVIDENCE_ARTIFACT_ID is a canonical positive decimal integer of at most 16 digits that does not exceed 9007199254740991"
    )]
    ValidateArtifactId,
    #[command(subcommand, about = "Decide the qualification workflow")]
    Qualification(QualificationCommand),
}

#[derive(Debug, Subcommand)]
pub enum SourcesCommand {
    #[command(
        about = "Run cargo fetch --locked in each Cargo root, in order, as velnor-ci run does for one command, so the cache report sees every fetch. Refuses a missing or duplicate root before any fetch, and stops at the first failing fetch, naming its root and exiting with cargo's exit code"
    )]
    Fetch(SourcesFetchArgs),
}

#[derive(Debug, Args)]
pub struct SourcesFetchArgs {
    #[arg(
        long,
        required = true,
        num_args = 1..,
        value_name = "DIR",
        help = "Cargo root directories, relative to the working directory or absolute, fetched in the order given"
    )]
    pub roots: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub struct FetchArgs {
    #[arg(
        long,
        required = true,
        help = "Cargo manifest to fetch; repeat for each Cargo root"
    )]
    pub manifest_path: Vec<PathBuf>,
    #[arg(
        long,
        hide = true,
        help = "An extra absolute directory whose programs and CARGO_HOME count as installed by the runner"
    )]
    pub trusted_dir: Vec<PathBuf>,
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

#[derive(Debug, Subcommand)]
pub enum TestGroupsCommand {
    #[command(
        about = "Run in the directory of a Cargo root after the tests are built: exit 1 and name every test that two or more groups select. Lists the workspace once per group with cargo-nextest, under the group's filter unchanged, so the filter means what the group job runs; ignored tests are exempt, and a listing error fails the check"
    )]
    Check(TestGroupsCheckArgs),
}

#[derive(Debug, Args)]
pub struct TestGroupsCheckArgs {
    #[arg(
        long = "group",
        required = true,
        value_name = "NAME=FILTER",
        value_parser = group_filter,
        help = "A group's name, an equals sign and its nextest filter; repeat for each group"
    )]
    pub groups: Vec<GroupFilter>,
}

#[derive(Debug, Clone)]
pub struct GroupFilter {
    pub name: String,
    pub filter: String,
}

fn group_filter(text: &str) -> Result<GroupFilter, String> {
    match text.split_once('=') {
        Some((name, filter)) if !name.trim().is_empty() && !filter.trim().is_empty() => {
            Ok(GroupFilter {
                name: name.to_owned(),
                filter: filter.to_owned(),
            })
        }
        _ => Err("expected NAME=FILTER with a name and a filter".to_owned()),
    }
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
pub struct CheckTagArgs {
    #[arg(help = "The release tag")]
    pub tag: String,
}

pub use crate::cli_release::{
    AlignArgs, CheckConfigArgs, FamilyTagArgs, PreflightArgs, PublishArgs, RecordArgs,
    ReleaseCommand, ReleaseGateArgs,
};
