// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;

use crate::OrchestratorError;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

const RELEASE_MANIFEST_REL: &str = ".velnor/release-manifest.json";

pub(crate) fn read_manifest_file(root: &Path) -> Result<Option<String>, OrchestratorError> {
    match read_repo_file(root, RELEASE_MANIFEST_REL, MAX_REPO_FILE_BYTES)? {
        RepoRead::Absent => Ok(None),
        RepoRead::Text(text) => Ok(Some(text)),
    }
}

pub(crate) fn for_policy(
    root: &Path,
    policy: WorkflowPolicy,
) -> Result<(Option<String>, bool), OrchestratorError> {
    match policy {
        WorkflowPolicy::ConsumerV1 => consumer_manifest_text_or_stand_in(root),
        WorkflowPolicy::VelnorRepositoryV1 => Ok((None, false)),
    }
}

#[cfg(debug_assertions)]
fn consumer_manifest_text_or_stand_in(
    root: &Path,
) -> Result<(Option<String>, bool), OrchestratorError> {
    if let Some(text) = read_manifest_file(root)? {
        return Ok((Some(text), false));
    }
    let sha = "a".repeat(64);
    let version = env!("CARGO_PKG_VERSION");
    let targets = [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ]
    .map(|target| {
        format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{sha}\"}}"
        )
    })
    .join(",");
    let commit = "b".repeat(40);
    Ok((
        Some(format!(
            "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{commit}\",\"targets\":[{targets}]}}"
        )),
        true,
    ))
}

#[cfg(not(debug_assertions))]
fn consumer_manifest_text_or_stand_in(
    root: &Path,
) -> Result<(Option<String>, bool), OrchestratorError> {
    read_manifest_file(root).map(|text| (text, false))
}
