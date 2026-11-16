// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::error::CiError;
use crate::release::PlainName;

#[derive(Debug, Args)]
pub struct ReleaseGateArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(long, help = "Full SHA of the commit that is about to be released")]
    pub sha: String,
    #[arg(
        allow_hyphen_values = true,
        long,
        help = "Required check enforced only on pull requests; repeat for each check"
    )]
    pub pr_only_check: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum ReleaseCommand {
    #[command(
        about = "Read what release-plz release -o json printed, one file per Cargo root, and print and write to $GITHUB_OUTPUT one line <package>=<tag> for each package it released (with --repo and --verified-commit, also one line <package>_commit=<sha> for the commit its tag is on, which must be the verified commit itself); exit 1 when a package or a tag appears twice, a package holds anything but ASCII letters, digits, '.', '_' and '-', or a tag holds anything but those and '+' (build metadata)"
    )]
    Record(RecordArgs),
    #[command(
        about = "Publish the draft release of a tag. Check each named file and SHA256SUMS in the directory as sha256sum --check --strict does, find the one draft release with the tag, which the GitHub App's bot must have created, upload the files and SHA256SUMS, and un-draft it only when it holds exactly those files with those digests; with --no-files, name and read no file and un-draft it only when it holds no asset. Exit 1 before any change on a missing, unlisted or changed file, on a published release or a second draft with the tag, on a draft that the App did not create, and on an asset that is not one of the files. Reads GITHUB_TOKEN, which needs contents: write"
    )]
    Publish(PublishArgs),
    #[command(
        about = "Read-only, before release-plz holds any publishing authority: exit 1 when release-plz release would tag a commit other than the verified one, which is when a pull request whose head branch starts with a given prefix is associated with the verified commit and its last commit is an ancestor of the verified commit other than that commit itself (a release pull request merged with a merge commit or fast-forwarded), and exit 1 when the pull requests cannot be read. Reads GITHUB_TOKEN, which needs contents: read and pull-requests: read"
    )]
    Preflight(PreflightArgs),
    #[command(
        about = "Parse each release-plz.toml and exit 1 when any table holds a replace_command or changelog_config key, however the key is written in TOML, or when a file is not valid TOML, or, with --tag-pattern and --repo, when a file's release tag patterns differ from the ones listed for it or the live tag rulesets of the repository do not cover them, or, with --branch-prefix, when a file's pr_branch_prefix differs from the one listed for it, or when a symbolic link or a cargo config that runs a program or holds a credential provider, proxy or CA override sits on the cargo config path of the folder of any file up to the working folder"
    )]
    CheckConfig(CheckConfigArgs),
}

#[derive(Debug, Args)]
pub struct CheckConfigArgs {
    #[arg(required = true, help = "release-plz.toml files to check")]
    pub files: Vec<PathBuf>,
    #[arg(
        allow_hyphen_values = true,
        long = "tag-pattern",
        num_args = 2,
        value_names = ["FILE", "PATTERN"],
        action = clap::ArgAction::Append,
        requires = "repo",
        help = "A git_tag_name pattern the tag rulesets were provisioned for; when given, every file must hold exactly the patterns listed for it"
    )]
    tag_pattern_words: Vec<String>,
    #[arg(
        allow_hyphen_values = true,
        long,
        value_name = "OWNER/NAME",
        requires = "tag_pattern_words",
        help = "With --tag-pattern, the repository whose active tag rulesets must hold a creation, update and deletion rule for every listed pattern"
    )]
    pub repo: Option<String>,
    #[arg(
        allow_hyphen_values = true,
        long = "branch-prefix",
        num_args = 2,
        value_names = ["FILE", "PREFIX"],
        action = clap::ArgAction::Append,
        help = "The pr_branch_prefix the workflow was generated for; when given, every file must hold exactly the prefix listed for it, release-plz-, its default, included"
    )]
    branch_prefix_words: Vec<String>,
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
    pub fn branch_prefixes(&self) -> Vec<(String, String)> {
        self.branch_prefix_words
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[file, prefix]| (file.clone(), prefix.clone()))
            .collect()
    }

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
    #[arg(
        allow_hyphen_values = true,
        long,
        value_name = "OWNER/NAME",
        requires = "verified_commit",
        help = "Repository whose tags to resolve; with --verified-commit"
    )]
    pub repo: Option<String>,
    #[arg(
        allow_hyphen_values = true,
        long,
        value_name = "SHA",
        requires = "repo",
        help = "Full SHA of the commit the release gate verified; also write <package>_commit=<sha> for each released package, the commit its tag is on, and exit 1 before writing anything when a tag is not on that commit itself. Reads GITHUB_TOKEN, which needs contents: read"
    )]
    pub verified_commit: Option<String>,
}

#[derive(Debug, Args)]
pub struct PreflightArgs {
    #[arg(long, value_name = "OWNER/NAME", help = "Repository as owner/name")]
    pub repo: String,
    #[arg(
        allow_hyphen_values = true,
        long,
        value_name = "SHA",
        help = "Full SHA of the commit the release gate verified"
    )]
    pub verified_commit: String,
    #[arg(
        long = "branch-prefix",
        required = true,
        allow_hyphen_values = true,
        value_name = "PREFIX",
        help = "A pr_branch_prefix of a release-plz root; give one per root"
    )]
    pub branch_prefixes: Vec<String>,
}

#[derive(Debug, Args)]
pub struct PublishArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(long, value_parser = tag, help = "Tag of the draft release")]
    pub tag: PlainName,
    #[arg(
        allow_hyphen_values = true,
        long,
        value_parser = app,
        help = "Slug of the GitHub App whose bot, <slug>[bot], created the draft release"
    )]
    pub app: PlainName,
    #[arg(
        allow_hyphen_values = true,
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
