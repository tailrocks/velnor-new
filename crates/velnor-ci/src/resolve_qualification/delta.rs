// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The source delta of a `useful_delta` run: what git says changed from the predecessor's
//! source to this run's source, after proving the first is an ancestor of the second.

use std::path::Path;
use std::process::Command;

use super::types::SourceDelta;
use super::validate::{canonical_bytes, digest_b3};
use crate::error::CiError;
use crate::write_request::internal;

const MAX_CHANGED_PATHS: usize = 256;
const MAX_DIFF_BYTES: usize = 1_048_576;

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
    let output = command
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|_| internal("qualification_git"))?;
    Ok(Output {
        success: output.status.success(),
        stdout: output.stdout,
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
