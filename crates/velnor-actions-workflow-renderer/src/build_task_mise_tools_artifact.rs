// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use crate::verification_jobs::build_task_jobs::BuildTaskArtifact;
use crate::verification_jobs::build_task_mise_tools::{locked_github_url, safe_identity};

pub(crate) fn validate_artifact(
    key: &str,
    artifact: &BuildTaskArtifact,
) -> Result<(), RenderError> {
    let checksum = artifact
        .checksum
        .strip_prefix("sha256:")
        .unwrap_or_default();
    if !velnor_actions_contract::ids::is_lower_hex_len(checksum, 64)
        || !locked_github_url(&artifact.url, "https://github.com/")
        || artifact
            .url_api
            .as_deref()
            .is_some_and(|url| !locked_github_url(url, "https://api.github.com/repos/"))
        || artifact
            .signer
            .as_deref()
            .is_some_and(|value| !safe_identity(value))
        || artifact
            .provenance
            .as_deref()
            .is_some_and(|value| !safe_identity(value))
        || artifact
            .checksum_source
            .as_deref()
            .is_some_and(|value| value != "mise-versions")
        || artifact.repository_ids.as_ref().is_some_and(|ids| {
            !ids.contains_key("repository")
                || ids
                    .keys()
                    .any(|key| !matches!(key.as_str(), "repository" | "owner"))
                || ids.values().any(|value| !safe_identity(value))
        })
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "build_task_artifact:{key}"
        )));
    }
    Ok(())
}
