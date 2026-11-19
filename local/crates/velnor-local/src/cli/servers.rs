// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Args;
use velnor_cache_rules::REPOSITORY_LIMIT_BYTES;

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
    #[arg(
        long,
        help = "runner.server as host:port; it answers the workflow run, job and job log endpoints"
    )]
    pub standin: Option<String>,
    #[arg(
        long,
        help = "Answer the download of tailrocks/velnor-new with the local variant of setup-velnor, which installs the velnor-ci that velnor-local mounted. `up` sets it with --velnor-ci-binary and nothing else does"
    )]
    pub local_setup_velnor: bool,
    #[arg(
        long,
        help = "Serve cache listing and deletion on the job port as well, for workflows whose cache maintenance job calls `velnor-ci cache budget`. Any bearer token is accepted there, so a job of any scope can list and delete the caches of every scope. Without it only the control port serves them and the job port answers 403"
    )]
    pub job_cache_admin: bool,
    #[command(flatten)]
    pub control: ControlArgs,
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
    #[arg(
        long,
        help = "Serve restores only and refuse every cache write, as GitHub does for pull requests from forks"
    )]
    pub restore_only: bool,
    #[command(flatten)]
    pub control: ControlArgs,
}

#[derive(Clone, Debug, Args)]
pub struct ControlArgs {
    #[arg(
        long,
        requires = "control_token",
        help = "Second address to listen on, where every request must carry the control token as a bearer token"
    )]
    pub control_listen: Option<SocketAddr>,
    #[arg(
        long,
        requires = "control_listen",
        help = "File that holds the control token"
    )]
    pub control_token: Option<PathBuf>,
}
