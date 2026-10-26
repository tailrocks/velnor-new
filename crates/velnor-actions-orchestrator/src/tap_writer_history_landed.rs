// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{BotIdentity, PullRequestResponse, WriterInputs};
use crate::{
    OrchestratorError,
    github_landed_pull::{RestLandedPull, RestPullRef, read_merge_oid},
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
    let head = pull
        .head
        .repo
        .as_ref()
        .ok_or_else(|| internal("tap_historical_pull_identity_mismatch"))?;
    let base = pull
        .base
        .repo
        .as_ref()
        .ok_or_else(|| internal("tap_historical_pull_identity_mismatch"))?;
    let rest = RestLandedPull {
        number: pull.number,
        state: &pull.state,
        merged: pull.merged == Some(true),
        merged_at,
        head: RestPullRef {
            reference: &pull.head.ref_name,
            sha: &pull.head.sha,
            repository_id: head.id,
            repository: &head.full_name,
        },
        base: RestPullRef {
            reference: &pull.base.ref_name,
            sha: &pull.base.sha,
            repository_id: base.id,
            repository: &base.full_name,
        },
        merge_sha: pull.merge_commit_sha.as_deref(),
    };
    let merge_oid = read_merge_oid(
        bot.repository_id,
        &inputs.policy.repository,
        &rest,
        |query, fields| inputs.api.graphql(query, fields),
    )?;
    Ok((merge_oid, merged_at.to_owned()))
}
