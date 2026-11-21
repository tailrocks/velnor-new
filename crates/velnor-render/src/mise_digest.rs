// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt;

use velnor_model::MisePlatform;

use crate::rules::{RenderedJob, Value};
use crate::steps::MISE_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiseBinary {
    pub version: &'static str,
    pub sha256: [(&'static str, &'static str); 4],
}

pub const MISE_BINARIES: &[MiseBinary] = &[MiseBinary {
    version: "2026.11.10",
    sha256: [
        (
            "Linux-ARM64",
            "86ef275d541c5243cd20b1c0fcd16810e5f3601113d8db09278d0608617a3833",
        ),
        (
            "Linux-X64",
            "b603a4f48563fc379a794d9ea2cf9f709ed276c9141068604cb0c59fa3e674a9",
        ),
        (
            "macOS-ARM64",
            "3f4ba9d21c6f2170048d0870fd98ab287e04730b76a6fc8e83964d59c80eb704",
        ),
        (
            "macOS-X64",
            "7eab8054d301b75dcbd93440ff184bee6864d95799cfdc4a00ce858313b68783",
        ),
    ],
}];

const NO_DIGEST: &str = "no-mise-digest-for-this-runner";

const fn same(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

const fn has_row(version: &str) -> bool {
    let mut index = 0;
    while index < MISE_BINARIES.len() {
        if same(MISE_BINARIES[index].version, version) {
            return true;
        }
        index += 1;
    }
    false
}

const _: () = assert!(
    has_row(MISE_VERSION),
    "MISE_VERSION has no row in MISE_BINARIES (crates/velnor-render/src/mise_digest.rs)"
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownMiseVersion(pub String);

impl fmt::Display for UnknownMiseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "mise {} has no known binary digest; add its row to MISE_BINARIES",
            self.0
        )
    }
}

impl Error for UnknownMiseVersion {}

pub fn mise_sha256_input(version: &str) -> Result<Value, UnknownMiseVersion> {
    MISE_BINARIES
        .iter()
        .find(|binary| binary.version == version)
        .map(selecting)
        .ok_or_else(|| UnknownMiseVersion(version.to_owned()))
}

pub(crate) fn pinned_sha256() -> Value {
    mise_sha256_input(MISE_VERSION).unwrap_or_else(|_| Value::Text(NO_DIGEST.to_owned()))
}

fn selecting(binary: &MiseBinary) -> Value {
    let chain: String = binary
        .sha256
        .iter()
        .filter(|(platform, _)| !platform.starts_with("Linux"))
        .filter_map(|(platform, digest)| {
            let (os, arch) = platform.split_once('-')?;
            Some(format!(
                "runner.os == '{os}' && runner.arch == '{arch}' && '{digest}' || "
            ))
        })
        .collect();
    Value::Expr(format!(
        "${{{{ runner.os != 'Windows' && runner.os != 'Linux' && ({chain}'{NO_DIGEST}') || '' }}}}"
    ))
}

pub(crate) fn pin_to_runner_platform(mut job: RenderedJob) -> RenderedJob {
    let Some(digest) = runner_digest(&job) else {
        return job;
    };
    for step in job.steps.iter_mut().filter(|step| {
        step.uses
            .as_deref()
            .is_some_and(|uses| uses.split('@').next() == Some(crate::rules::MISE_ACTION))
    }) {
        for (key, value) in &mut step.with {
            if key == "sha256" && *value == pinned_sha256() {
                *value = Value::Text(digest.to_owned());
            }
        }
    }
    job
}

fn runner_digest(job: &RenderedJob) -> Option<&'static str> {
    if job.runs_on_axis.is_some() || job.fork_runner.is_some() || job.trusted_runner.is_some() {
        return None;
    }
    let platform = plain_label_platform(&job.runs_on)?;
    let binary = MISE_BINARIES
        .iter()
        .find(|binary| binary.version == MISE_VERSION)?;
    binary
        .sha256
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&platform))
        .map(|(_, digest)| *digest)
}

fn plain_label_platform(label: &str) -> Option<String> {
    label
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
        .then_some(())?;
    let platform = MisePlatform::runner(label)?;
    (!platform.matches("linux") || label.starts_with("ubuntu-")).then(|| platform.lock_key())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RenderedTrusted;

    #[test]
    fn a_trusted_pool_behind_an_ubuntu_label_gets_no_glibc_digest_without_a_fork_fence() {
        let job = RenderedJob {
            runs_on: "ubuntu-26.04-scale-set".to_owned(),
            trusted_runner: Some(RenderedTrusted {
                label: "ubuntu-26.04-scale-set-musl".to_owned(),
                replaces: "ubuntu-26.04-scale-set".to_owned(),
            }),
            ..RenderedJob::default()
        };
        assert_eq!(runner_digest(&job), None);
    }
}
