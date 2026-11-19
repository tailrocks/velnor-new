// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::types::SourceDelta;
use super::validate::{canonical_bytes, digest_b3};
use crate::bounded::capture;
use crate::error::CiError;
use crate::write_request::internal;

const MAX_CHANGED_PATHS: usize = 256;
const MAX_DIFF_BYTES: usize = 1_048_576;

const GIT_TIMEOUT: Duration = Duration::from_secs(600);

struct Output {
    success: bool,
    stdout: Vec<u8>,
}

fn git(root: &Path, args: &[&str]) -> Result<Output, CiError> {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    let limit = u64::try_from(MAX_DIFF_BYTES).unwrap_or(u64::MAX);
    let captured =
        capture(command.current_dir(root).args(args), limit, GIT_TIMEOUT).map_err(|error| {
            internal(if error.kind() == ErrorKind::TimedOut {
                "qualification_git_timeout"
            } else {
                "qualification_git"
            })
        })?;
    Ok(Output {
        success: captured.success,
        stdout: captured.stdout,
    })
}

fn validate_sha(value: &str) -> Result<(), CiError> {
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(internal("qualification_source_sha_invalid"))
    }
}

pub(super) fn derive(root: &Path, base: &str, source: &str) -> Result<SourceDelta, CiError> {
    validate_sha(base)?;
    validate_sha(source)?;
    if !git(root, &["merge-base", "--is-ancestor", base, source])?.success {
        return Err(internal("qualification_source_not_ancestor"));
    }
    let diff = git(
        root,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            base,
            source,
            "--",
        ],
    )?;
    if !diff.success || diff.stdout.len() > MAX_DIFF_BYTES {
        return Err(internal("qualification_diff_unavailable"));
    }
    let mut paths = diff
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|chunk| String::from_utf8(chunk.to_vec()))
        .collect::<Result<Vec<String>, _>>()
        .map_err(|_| internal("qualification_diff_non_utf8"))?;
    paths.sort();
    paths.dedup();
    if paths.is_empty() || paths.len() > MAX_CHANGED_PATHS {
        return Err(internal("qualification_source_delta_path_count"));
    }
    let diff_digest = digest_b3(&canonical_bytes(&paths)?);
    Ok(SourceDelta {
        base_source_sha: base.to_owned(),
        source_sha: source.to_owned(),
        changed_paths: paths,
        diff_digest,
        base_is_ancestor: true,
    })
}
