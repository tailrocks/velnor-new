// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_model::{JOB_AUTHORITY_CONFIG_PATH, RepoPath};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobAuthorityError {
    Config(String),
    NotGenerated(RepoPath),
    NoGeneratorDigest,
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
            Self::NoGeneratorDigest => write!(
                f,
                "the job-authority inventory names the pinned velnor-gen release, but .velnor/rust-ci.lock pins no linux-arm64 velnor-gen archive; pin a release that lists one with `velnor-gen pin-helper <version>`"
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
