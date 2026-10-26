use super::{BotIdentity, PullRequestResponse, WriterInputs};
use crate::{
    OrchestratorError,
    github_landed_pull::{RestLandedPull, request_merge_oid},
    internal::internal,
};

pub(super) fn merge_oid(
    inputs: &WriterInputs,
    bot: &BotIdentity,
    pull: &PullRequestResponse,
) -> Result<(String, String), OrchestratorError> {
    let merged_at = pull
        .merged_at
        .as_deref()
        .filter(|value| super::timestamp::valid_timestamp(value))
        .ok_or_else(|| internal("tap_historical_merge_time_missing"))?;
    let head_repository = pull
        .head
        .repo
        .as_ref()
        .ok_or_else(|| internal("tap_historical_pull_identity_mismatch"))?;
    let base_repository = pull
        .base
        .repo
        .as_ref()
        .ok_or_else(|| internal("tap_historical_pull_identity_mismatch"))?;
    let rest = RestLandedPull {
        number: pull.number,
        state: &pull.state,
        merged: pull.merged == Some(true),
        merged_at,
        head_ref: &pull.head.ref_name,
        head_sha: &pull.head.sha,
        head_repository_id: head_repository.id,
        head_repository: &head_repository.full_name,
        base_ref: &pull.base.ref_name,
        base_sha: &pull.base.sha,
        base_repository_id: base_repository.id,
        base_repository: &base_repository.full_name,
    };
    let merge_oid = request_merge_oid(
        bot.repository_id,
        &inputs.policy.repository,
        &rest,
        |query, fields| inputs.api.graphql(query, fields),
    )?;
    Ok((merge_oid, merged_at.to_owned()))
}
