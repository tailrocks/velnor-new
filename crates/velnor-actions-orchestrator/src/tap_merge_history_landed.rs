use serde_json::Value;

use crate::{OrchestratorError, github_landed_pull::RestLandedPull};

use super::{MergeAuthority, MergeCandidate, MergeProjection, TapMergeGitHub};

pub(super) fn read_merge_sha(
    candidate: &MergeCandidate,
    authority: &MergeAuthority,
    api: &TapMergeGitHub,
    detail: &Value,
    projection: &MergeProjection<'_>,
) -> Result<Option<String>, OrchestratorError> {
    let Some(merged_at) = projection.merged_at else {
        return Ok(None);
    };
    let head_repository = super::required_text(
        detail,
        "/head/repo/full_name",
        "tap_merge_history_pr_identity",
    )?;
    let base_repository = super::required_text(
        detail,
        "/base/repo/full_name",
        "tap_merge_history_pr_identity",
    )?;
    let merge_sha = crate::github_landed_pull::request_merge_oid(
        authority.repo_id,
        &candidate.policy.repository,
        &RestLandedPull {
            number: projection.number,
            state: super::required_text(detail, "/state", "tap_merge_history_pr_identity")?,
            merged: projection.merged,
            merged_at,
            head_ref: projection.branch,
            head_sha: projection.head_sha,
            head_repository_id: authority.repo_id,
            head_repository,
            base_ref: super::required_text(detail, "/base/ref", "tap_merge_history_pr_identity")?,
            base_sha: projection.observed_base_sha,
            base_repository_id: authority.repo_id,
            base_repository,
        },
        |query, fields| api.graphql(query, fields),
    )?;
    Ok(Some(merge_sha))
}
