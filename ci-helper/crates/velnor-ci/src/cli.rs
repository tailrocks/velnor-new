// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use velnor_model::{InvalidId, RepoPath};

use crate::error::CiError;
use crate::release::PlainName;

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
    #[command(subcommand, about = "Collect and compare run metrics")]
    Bench(BenchCommand),
    #[command(
        subcommand,
        about = "Keep tracked files' modification times with a cached target directory, and audit the files packages read at compile time"
    )]
    Inputs(InputsCommand),
    #[command(
        about = "Print and write to $GITHUB_OUTPUT ready=true when the commit is on the default branch and all required checks outside the explicit --pr-only-check list passed in push jobs on that branch at the exact commit; ready=false while a check is unfinished; exit 1 on a failed check, invalid exemption or off-branch commit. Reads GITHUB_TOKEN, which needs contents: read, actions: read and statuses: read"
    )]
    ReleaseGate(ReleaseGateArgs),
    #[command(
        about = "The required check: read the needs context from NEEDS_JSON, and the spec in the variable --spec-env names, and the plan outputs of the plan job the spec names, then exit 1 unless every job the plan scheduled succeeded, no job it did not schedule failed, and every package that listed tests in a selected test job ran at least one. Fails when the plan job failed, a plan output is missing or invalid, a job is missing from needs, or a result is cancelled or unknown. Prints a table and appends it to $GITHUB_STEP_SUMMARY"
    )]
    Gate(GateArgs),
    #[command(about = "Exit 1 unless the tag is rust-ci-v followed by the version of this program")]
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
        about = "Run after the tools are installed: exit 1 naming each tool that mise does not report at its pinned version, append RUSTUP_AUTO_INSTALL=0 and the CI build profile to $GITHUB_ENV, and write the step output compat to $GITHUB_OUTPUT; run in the checkout"
    )]
    Prepare,
    #[command(
        about = "Run after the tools are installed, in the directory of a Cargo root: exit 1 unless .velnor/rust-metadata.json is exactly what velnor-gen generate writes for that root, which reads cargo metadata --no-deps --offline"
    )]
    CheckMetadata,
    #[command(
        about = "Resolve every requested feature configuration afresh, then cargo check each distinct full package feature map once, offline and locked"
    )]
    CheckConfigurations(CheckConfigurationsArgs),
    #[command(
        about = "Run after every cache and after velnor-ci inputs restore: every action that must follow the restores. Gives each tracked file that shares its modification time with another tracked file a distinct time, one microsecond below the time it had and ordered by path, so that cargo, which picks the newest file of a package by time alone, names the same file after every restore"
    )]
    Restored,
    #[command(
        about = "Run a command, stream its output unchanged, append it to velnor-run.log in $RUNNER_TEMP, and exit with its exit code"
    )]
    Run(RunArgs),
    #[command(
        about = "Write .velnor/rust-ci-timings.json from the last successful runs of the generated workflow on the default branch: the median of at least three ordinary runs, one entry per Cargo root, counting only jobs that ran on the runner in .velnor/rust-ci.toml (default ubuntu-latest); writes nothing with fewer; reads GITHUB_TOKEN; run in the checkout"
    )]
    Timings(TimingsArgs),
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
pub struct GateArgs {
    #[arg(
        long,
        value_name = "VARIABLE",
        help = "The environment variable that holds the gate spec the generator wrote: --plan <job>, then --job <ID=WHEN> for every job of needs (WHEN is always, or conditions joined by +: lane:<root>, policy, schedule, schedule-or-push) and --tests <ID=ROOT> for each job whose tests_run output counts the tests of the root"
    )]
    pub spec_env: String,
}

#[derive(Debug, Args)]
pub struct CheckTagArgs {
    #[arg(help = "The release tag")]
    pub tag: String,
}

#[derive(Debug, Args)]
pub struct ReleaseGateArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(long, help = "Full SHA of the commit that is about to be released")]
    pub sha: String,
    #[arg(
        long,
        help = "Required check enforced only on pull requests; repeat for each check"
    )]
    pub pr_only_check: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum ReleaseCommand {
    #[command(
        about = "Read what release-plz release -o json printed, one file per Cargo root, and print and write to $GITHUB_OUTPUT one line <package>=<tag> for each package it released; exit 1 when a package or a tag appears twice, a package holds anything but ASCII letters, digits, '.', '_' and '-', or a tag holds anything but those and '+' (build metadata)"
    )]
    Record(RecordArgs),
    #[command(
        about = "Publish the draft release of a tag. Check each named file and SHA256SUMS in the directory as sha256sum --check --strict does, find the one draft release with the tag, which the GitHub App's bot must have created, upload the files and SHA256SUMS, and un-draft it only when it holds exactly those files with those digests; with --no-files, name and read no file and un-draft it only when it holds no asset. Exit 1 before any change on a missing, unlisted or changed file, on a published release or a second draft with the tag, on a draft that the App did not create, and on an asset that is not one of the files. Reads GITHUB_TOKEN, which needs contents: write"
    )]
    Publish(PublishArgs),
    #[command(
        about = "Parse each release-plz.toml and exit 1 when any table holds a replace_command or changelog_config key, however the key is written in TOML, or when a file is not valid TOML, or, with --tag-pattern and --repo, when a file's release tag patterns differ from the ones listed for it or the live tag rulesets of the repository do not cover them, or when a symbolic link or a cargo config that runs a program or holds a credential provider, proxy or CA override sits on the cargo config path of the folder of any file up to the working folder"
    )]
    CheckConfig(CheckConfigArgs),
}

#[derive(Debug, Args)]
pub struct CheckConfigArgs {
    #[arg(required = true, help = "release-plz.toml files to check")]
    pub files: Vec<PathBuf>,
    #[arg(
        long = "tag-pattern",
        num_args = 2,
        value_names = ["FILE", "PATTERN"],
        action = clap::ArgAction::Append,
        requires = "repo",
        help = "A git_tag_name pattern the tag rulesets were provisioned for; when given, every file must hold exactly the patterns listed for it"
    )]
    tag_pattern_words: Vec<String>,
    #[arg(
        long,
        value_name = "OWNER/NAME",
        requires = "tag_pattern_words",
        help = "With --tag-pattern, the repository whose active tag rulesets must hold a creation, update and deletion rule for every listed pattern"
    )]
    pub repo: Option<String>,
    #[arg(
        long,
        help = "Read the Cargo workspace of each file's folder with cargo metadata --no-deps --offline, which runs no repository code, and refuse a released package whose package.publish names a registry other than crates-io, a [[package]] that is not a member of its workspace, or a package that two roots release; needs RUSTUP_TOOLCHAIN"
    )]
    pub manifests: bool,
    #[arg(
        long,
        requires = "manifests",
        help = "With --manifests, also refuse a package whose current version would give a tag git cannot create; only for the job that tags the version already committed"
    )]
    pub released_versions: bool,
}

impl CheckConfigArgs {
    #[must_use]
    pub fn tag_patterns(&self) -> Vec<(String, String)> {
        self.tag_pattern_words
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[file, pattern]| (file.clone(), pattern.clone()))
            .collect()
    }
}

#[derive(Debug, Args)]
pub struct RecordArgs {
    #[arg(
        required = true,
        help = "Files that hold the JSON release-plz release -o json printed"
    )]
    pub outputs: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub struct PublishArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(long, value_parser = tag, help = "Tag of the draft release")]
    pub tag: PlainName,
    #[arg(
        long,
        value_parser = app,
        help = "Slug of the GitHub App whose bot, <slug>[bot], created the draft release"
    )]
    pub app: PlainName,
    #[arg(
        long,
        default_value = ".",
        help = "Directory that holds the files and SHA256SUMS"
    )]
    pub dir: PathBuf,
    #[arg(
        long,
        conflicts_with = "files",
        help = "Publish a release that must hold no asset at all; no file or SHA256SUMS is read"
    )]
    pub no_files: bool,
    #[arg(
        required_unless_present = "no_files",
        value_parser = file_name,
        help = "Exact names of the files to publish besides SHA256SUMS"
    )]
    pub files: Vec<PlainName>,
}

fn tag(text: &str) -> Result<PlainName, CiError> {
    PlainName::tag(text)
}

fn app(text: &str) -> Result<PlainName, CiError> {
    PlainName::new("App slug", text)
}

fn file_name(text: &str) -> Result<PlainName, CiError> {
    PlainName::new("file", text)
}

#[derive(Debug, Subcommand)]
pub enum InputsCommand {
    #[command(
        about = "Write target/.velnor/inputs.tsv: path, blob id of the bytes on disk, modification time in nanoseconds and mode of every regular tracked file in the worktree; paths a TSV field cannot hold are counted and left out"
    )]
    Save,
    #[command(
        about = "Set the saved modification time on every tracked file whose path, bytes on disk and mode equal the saved ones; report changed, new, deleted, absent and left-out files"
    )]
    Restore,
    #[command(
        about = "Run after a build of all targets in every Cargo root: read cargo's dep-info and build script output, and exit 1 when .velnor/rust-inputs.lock does not list a file that a package read at compile time from outside its own directory, lists a line nothing reads, or the build is incomplete: every unit an all-targets build makes (each target, its test harness when it is tested, each build script run, and the library and build script run of each path dependency outside the workspaces wherever cargo built it) needs a current unit, identified by cargo's fingerprint, in each dev-profile directory (debug or <triple>/debug) that holds a test harness of a member, with proc-macro libraries in the host debug directory (targets that need features are skipped). A unit with a cargo fingerprint and no dep-info, and Cargo roots whose target directories hold one another, are refused. Only the features, target and profile of that build are seen. Files that tests read at run time, and files a build script reads without naming them with cargo:rerun-if-changed, are not seen here; declare them with [[inputs]] in .velnor/rust-ci.toml"
    )]
    Audit(AuditArgs),
}

#[derive(Debug, Args)]
pub struct AuditArgs {
    #[arg(
        long,
        help = "Write .velnor/rust-inputs.lock, one path<TAB>package line per read, instead of checking it"
    )]
    pub write: bool,
}

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
}
