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
        long = "bin",
        required = true,
        help = "A binary to stage; repeat for each"
    )]
    pub bins: Vec<String>,
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
