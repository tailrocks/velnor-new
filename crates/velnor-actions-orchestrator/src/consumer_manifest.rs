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
) -> Result<Option<String>, OrchestratorError> {
    match policy {
        WorkflowPolicy::ConsumerV1 => read_manifest_file(root),
        WorkflowPolicy::VelnorRepositoryV1 => Ok(None),
    }
}
