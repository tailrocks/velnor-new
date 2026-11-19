// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use velnor_model::{InvalidId, RepoPath};

#[derive(Debug, Subcommand)]
pub enum QualificationCommand {
    #[command(
        about = "Read the needs context from NEEDS_JSON and exit 1 unless every --lane job succeeded, which is the verdict of the qualification compare job. Fails when NEEDS_JSON is unset or invalid, a lane is missing from needs, or a lane result is missing, skipped, cancelled, failure or unknown. Prints a table and appends it to $GITHUB_STEP_SUMMARY"
    )]
    Compare(QualificationCompareArgs),
}

#[derive(Debug, Args)]
pub struct QualificationCompareArgs {
    #[arg(
        long,
        default_values = ["verify-hosted", "verify-scale-set"],
        help = "Job id of a lane the compare job needs; repeat for each lane"
    )]
    pub lane: Vec<String>,
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
        long,
        value_enum,
        requires = "request",
        help = "Write the plan contract of a plan request instead of the Rust selection: ci for a push or merge_group run, qualification for a qualification run"
    )]
    pub mode: Option<PlanMode>,
    #[arg(
        long,
        value_name = "FILE",
        requires = "mode",
        conflicts_with_all = ["changed", "explain", "full", "base_files"],
        help = "The plan request, as velnor-ci write-request wrote it. Writes plan_id, run_key, not_affected_jobs and the qualification_* values to $GITHUB_OUTPUT, and runs neither cargo nor mise. The run key comes from the request, else from $GITHUB_RUN_ID and $VELNOR_RUN_KEY_ATTEMPT or $GITHUB_RUN_ATTEMPT. A pull_request, fork or local request lists in not_affected_jobs the rust-* and check-* jobs its diff does not reach, read from the git checkout in the working directory, .velnor/config.toml, the manifests and the request's lane inventory (which it requires); when git cannot say what changed every job runs. A qualification phase that has a predecessor is refused"
    )]
    pub request: Option<PathBuf>,
    #[arg(
        long,
        value_name = "BRANCH",
        requires = "request",
        help = "The default branch a qualification run must be on; required with --mode qualification"
    )]
    pub default_branch: Option<String>,
    #[arg(
        long = "base-file",
        value_name = "PATH=FILE",
        value_parser = base_file,
        help = "The text a changed Cargo.lock, Cargo.toml or configuration file had before the change: the repository path, an equals sign, and a file holding the old text. Without it a changed Cargo.lock or root Cargo.toml selects everything, and a changed file that makes a check do less (.velnor/rust-ci.toml, a Cargo.toml, a cargo, mise or nextest configuration) counts as adding every such setting it holds, which the run reports as a warning. Repeat for each file"
    )]
    pub base_files: Vec<BaseFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PlanMode {
    Ci,
    Qualification,
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

#[derive(Debug, Subcommand)]
pub enum BenchCommand {
    #[command(about = "Turn a workflow run into a metrics file; reads GITHUB_TOKEN")]
    Collect(CollectArgs),
    #[command(about = "Print a markdown table comparing two metrics files")]
    Compare(CompareArgs),
    #[command(
        about = "Compare the speed budget of .velnor/rust-ci.toml with the last successful runs of the generated workflow: read the budget, collect the last --last successful attempt-1 runs of each event, compare their medians and their latest run with the budget, print the three jobs whose duration is furthest above their median, and exit 1 on a breach or on a measurement the budget needs but the runs do not have. Reads GITHUB_TOKEN, which needs actions: read; run in the checkout"
    )]
    Budget(BudgetArgs),
}

#[derive(Debug, Args)]
pub struct BudgetArgs {
    #[arg(
        long,
        default_value_t = 10,
        help = "How many successful runs of each event to measure"
    )]
    pub last: usize,
    #[arg(long, help = "Repository as owner/name")]
    pub repo: Option<String>,
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

#[derive(Debug, Args)]
pub struct ToolsLocalArgs {
    #[arg(value_enum)]
    pub verb: crate::tools_local::Verb,
    #[arg(long)]
    pub key: String,
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
