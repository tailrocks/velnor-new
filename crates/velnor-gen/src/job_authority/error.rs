// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_model::{JOB_AUTHORITY_CONFIG_PATH, RepoPath};
use velnor_rust::LockError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobAuthorityError {
    Config(String),
    NotGenerated(RepoPath),
    NotPinned,
    NoDigest,
    NotLinux(Option<&'static str>),
    Lock(LockError),
    TooManyWorkflows { listed: usize, limit: usize },
    CommitMismatch { running: String, pinned: String },
    Workflow(RepoPath, String),
    Render(String),
}

impl fmt::Display for JobAuthorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(reason) => write!(f, "{JOB_AUTHORITY_CONFIG_PATH}: {reason}"),
            Self::NotGenerated(path) => write!(
                f,
                "{JOB_AUTHORITY_CONFIG_PATH} lists {path}, which velnor-gen does not generate"
            ),
            Self::NotPinned => write!(
                f,
                "the job-authority inventory names the pinned velnor-gen release, but .velnor/rust-ci.lock pins none; pin one with `velnor-gen pin-helper <version>`"
            ),
            Self::NoDigest => write!(
                f,
                "this velnor-gen cannot hash its own executable, so it cannot name its digest for the inventory"
            ),
            Self::NotLinux(platform) => write!(
                f,
                "the job-authority inventory is written only by a linux velnor-gen, because .velnor/rust-ci.lock pins the linux binaries only; this velnor-gen runs on {}; run `velnor-gen generate` in CI or on an OrbStack linux machine",
                platform.unwrap_or("an unsupported platform")
            ),
            Self::Lock(error) => write!(
                f,
                "this velnor-gen is not the binary .velnor/rust-ci.lock pins: {error}"
            ),
            Self::TooManyWorkflows { listed, limit } => write!(
                f,
                "{JOB_AUTHORITY_CONFIG_PATH} lists {listed} workflows, but velnor-host accepts at most {limit}"
            ),
            Self::CommitMismatch { running, pinned } => write!(
                f,
                "the job-authority inventory is written only by the pinned velnor-gen release: this velnor-gen is commit {running}, but .velnor/rust-ci.lock pins commit {pinned}; run the velnor-gen of the pinned release"
            ),
            Self::Workflow(path, reason) => write!(f, "{path}: {reason}"),
            Self::Render(reason) => {
                write!(f, "the job-authority inventory cannot be written: {reason}")
            }
        }
    }
}

impl std::error::Error for JobAuthorityError {}
