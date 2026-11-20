// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The SHA-256 of the mise binary every generated `jdx/mise-action` step installs.
//!
//! The action's `sha256` input compares the installed executable with one digest, and a non-empty
//! input makes the action skip its signed-archive check. That check also covers `mise-shim.exe`,
//! which the Windows archive ships beside `mise.exe` and which only the signed check verifies.
//! Windows therefore carries no digest: its selection is empty and the signed check runs. Linux
//! carries none either, unless the job runs on a GitHub-hosted glibc label: a musl runner downloads
//! a different binary, so a glibc digest would fail its check, and `runner.os` cannot tell the two
//! apart. A GitHub-hosted job (no fork runner, no trusted runner) whose label names one platform
//! gets that platform's digest as a literal; a hosted `ubuntu-*` label is glibc, while a label that
//! only names Linux is not. A self-hosted label proves no platform, because any name can be
//! registered: `ubuntu-musl` is a musl runner. Any other job (a self-hosted label, an unknown
//! label, a matrix runner, a fork runner of another architecture) keeps a run-time
//! selection by `runner.os` and `runner.arch`, which is longer (about 500 bytes a step, and a
//! workflow of many roots has to stay under the 500 000 byte limit). A macOS platform without a
//! row selects a text that matches no binary, so the action fails with a SHA256 mismatch instead
//! of installing an unchecked binary.
//!
//! The table is checked in, next to [`MISE_VERSION`], because Velnor owns the mise pin (mise is
//! not a project-declared catalog tool, see hard decision 4). A version without a row does not
//! build: the assertion below fails at compile time. `scripts/mise-binary-digests.sh` rewrites
//! the row from the release's official `SHASUMS256.txt` and checks it against the same file.

use std::error::Error;
use std::fmt;

use velnor_model::MisePlatform;

use crate::rules::{RenderedJob, Value};
use crate::steps::MISE_VERSION;

/// The digests of one mise release. `sha256` maps `<runner.os>-<runner.arch>` to the lower-case
/// SHA-256 of the raw executable `mise-v<version>-<os>-<arch>` of that release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiseBinary {
    pub version: &'static str,
    pub sha256: [(&'static str, &'static str); 4],
}

/// Every mise release the renderer can pin. Rewrite the row with
/// `scripts/mise-binary-digests.sh <version> --write` when `MISE_VERSION` changes.
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

/// What a platform without a digest selects: no binary has this SHA-256.
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

/// The pinned mise version has no known digest.
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

/// The `sha256` input that pins `version` of mise, or an error when no digest is known for it.
pub fn mise_sha256_input(version: &str) -> Result<Value, UnknownMiseVersion> {
    MISE_BINARIES
        .iter()
        .find(|binary| binary.version == version)
        .map(selecting)
        .ok_or_else(|| UnknownMiseVersion(version.to_owned()))
}

/// The `sha256` input of the pinned mise version. `MISE_VERSION` always has a row (compile-time
/// assertion above); the fallback text fails the action's check rather than skipping it.
pub(crate) fn pinned_sha256() -> Value {
    mise_sha256_input(MISE_VERSION).unwrap_or_else(|_| Value::Text(NO_DIGEST.to_owned()))
}

/// One comparison chain: no `fromJSON`, no matrix, so the workflow stays straight-line text.
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

/// Narrows the run-time digest selection of every mise step of `job` to the one digest of the
/// job's runner platform, when the labels of the job name exactly one platform.
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

/// `<os>-<arch>` of a plain runner label such as `ubuntu-26.04-arm`; no expression, no guess.
/// A Linux label that is not `ubuntu-*` names no libc, so it has no platform here.
fn plain_label_platform(label: &str) -> Option<String> {
    label
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
        .then_some(())?;
    let platform = MisePlatform::runner(label)?;
    (!platform.matches("linux") || label.starts_with("ubuntu-")).then(|| platform.lock_key())
}
