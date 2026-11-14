// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use velnor_cache_rules::REPOSITORY_LIMIT_BYTES;

#[derive(Debug, Parser)]
#[command(name = "velnor-local", version, about = "The local GitHub environment")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(
        about = "Serve GitHub's cache protocol from local disk under GitHub's cache rules, and forward every other request to runner.server"
    )]
    CacheServer(CacheServerArgs),
    #[command(
        about = "Answer the GitHub API calls velnor-ci makes from a local git repository, a file of open pull requests and the cache server's index"
    )]
    ApiStub(ApiStubArgs),
    #[command(
        about = "Start runner.server, the cache server, the API stub and linux/arm64 runner containers for one state directory"
    )]
    Up(UpArgs),
    #[command(about = "Stop and remove the containers and the network of one state directory")]
    Down(DownArgs),
    #[command(
        about = "Start an environment, replay a scenario against a copy of a local repository, stop the environment and print a verdict"
    )]
    Run(RunArgs),
}

#[derive(Debug, Args)]
pub struct EnvironmentArgs {
    #[arg(
        long,
        help = "State directory of this environment: keys, cache, API stub state, events and logs"
    )]
    pub dir: PathBuf,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u16).range(1..), help = "Number of runner containers, at least one")]
    pub runners: u16,
    #[arg(
        long,
        default_value = "main",
        help = "Default branch of the repository"
    )]
    pub default_branch: String,
}

#[derive(Debug, Args)]
pub struct UpArgs {
    #[command(flatten)]
    pub environment: EnvironmentArgs,
    #[arg(long, help = "Local git repository that the API stub reads")]
    pub repo: PathBuf,
}

#[derive(Debug, Args)]
pub struct DownArgs {
    #[arg(long, help = "State directory of the environment")]
    pub dir: PathBuf,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(help = "Scenario name in --scenarios, or the path of a scenario file")]
    pub scenario: String,
    #[command(flatten)]
    pub environment: EnvironmentArgs,
    #[arg(
        long,
        help = "Local git checkout; it is copied with its uncommitted changes and never modified"
    )]
    pub repo: PathBuf,
    #[arg(
        long,
        default_value = "local/scenarios",
        help = "Directory of scenario files"
    )]
    pub scenarios: PathBuf,
    #[arg(
        long,
        help = "Workflow file relative to the repository; every workflow runs when omitted"
    )]
    pub workflow: Option<String>,
    #[arg(
        long,
        default_value = "local/repository",
        help = "Repository as owner/name, as github.repository reports it"
    )]
    pub repository: String,
}

#[derive(Debug, Args)]
pub struct ApiStubArgs {
    #[arg(long, default_value = "127.0.0.1:80", help = "Address to listen on")]
    pub listen: SocketAddr,
    #[arg(long, help = "Local git repository that answers every repository path")]
    pub repo: PathBuf,
    #[arg(
        long,
        help = "State directory that holds pulls.json, the open pull requests"
    )]
    pub dir: PathBuf,
    #[arg(
        long,
        help = "The cache server's directory, which holds index.json and archives/"
    )]
    pub cache_dir: PathBuf,
    #[arg(
        long,
        default_value = "main",
        help = "Branch that a contents request without ref reads"
    )]
    pub default_branch: String,
    #[arg(
        long,
        default_value_t = 256 << 20,
        help = "Largest diff in bytes that pull request files and compare read; a larger one is a 422"
    )]
    pub diff_limit_bytes: usize,
}

#[derive(Debug, Args)]
pub struct CacheServerArgs {
    #[arg(long, default_value = "127.0.0.1:80", help = "Address to listen on")]
    pub listen: SocketAddr,
    #[arg(
        long,
        help = "runner.server as host:port; every request that is not a cache call goes there"
    )]
    pub upstream: String,
    #[arg(long, help = "Directory that holds the archives and the index file")]
    pub dir: PathBuf,
    #[arg(
        long,
        help = "Public half of runner.server's token signing key, PKCS#1 RSAPublicKey DER"
    )]
    pub token_key: PathBuf,
    #[arg(long, default_value_t = REPOSITORY_LIMIT_BYTES, help = "Cache size limit of one repository in bytes")]
    pub limit_bytes: u64,
}
