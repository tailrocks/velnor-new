// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::{OrchestratorError, internal::internal};

#[path = "github_landed_pull_types.rs"]
mod types;
use types::{GraphqlEnvelope, GraphqlPullRequest, GraphqlRepository};

const QUERY: &str = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){fullDatabaseId nameWithOwner pullRequest(number:$number){number state merged mergedAt headRefName headRefOid baseRefName baseRefOid headRepository{fullDatabaseId nameWithOwner} mergeCommit{oid}}}}";

pub(crate) struct RestPullRef<'a> {
    pub(crate) reference: &'a str,
    pub(crate) sha: &'a str,
    pub(crate) repository_id: u64,
    pub(crate) repository: &'a str,
}

pub(crate) struct RestLandedPull<'a> {
    pub(crate) number: u64,
    pub(crate) state: &'a str,
    pub(crate) merged: bool,
    pub(crate) merged_at: &'a str,
    pub(crate) head: RestPullRef<'a>,
    pub(crate) base: RestPullRef<'a>,
    pub(crate) merge_sha: Option<&'a str>,
}

pub(crate) fn read_merge_oid(
    repository_id: u64,
    repository: &str,
    rest: &RestLandedPull<'_>,
    request: impl FnOnce(&str, &[(&str, String)]) -> Result<Value, OrchestratorError>,
) -> Result<String, OrchestratorError> {
    validate_rest(repository_id, repository, rest)?;
    let (owner, name) = repository.split_once('/').ok_or_else(mismatch)?;
    let fields = [
        ("owner", owner.to_owned()),
        ("name", name.to_owned()),
        ("number", rest.number.to_string()),
    ];
    let response = request(QUERY, &fields)?;
    if response
        .get("errors")
        .is_some_and(|errors| !errors.as_array().is_some_and(Vec::is_empty))
    {
        return Err(mismatch());
    }
    let envelope: GraphqlEnvelope = serde_json::from_value(response).map_err(|_| mismatch())?;
    let observed = envelope
        .data
        .and_then(|data| data.repository)
        .ok_or_else(mismatch)?;
    validate_graphql(observed, repository_id, repository, rest)
}

fn validate_rest(
    repository_id: u64,
    repository: &str,
    rest: &RestLandedPull<'_>,
) -> Result<(), OrchestratorError> {
    if repository_id == 0
        || !velnor_actions_contract::vcs::is_safe_github_repository(repository)
        || rest.number == 0
        || i32::try_from(rest.number).is_err()
        || rest.state != "closed"
        || !rest.merged
        || rest.merged_at.is_empty()
        || rest.head.repository_id != repository_id
        || rest.head.repository != repository
        || rest.base.repository_id != repository_id
        || rest.base.repository != repository
        || rest.head.reference.is_empty()
        || rest.base.reference.is_empty()
        || !valid_sha(rest.head.sha)
        || !valid_sha(rest.base.sha)
        || rest.merge_sha.is_some_and(|sha| !valid_sha(sha))
    {
        return Err(mismatch());
    }
    Ok(())
}

fn validate_graphql(
    observed: GraphqlRepository,
    repository_id: u64,
    repository: &str,
    rest: &RestLandedPull<'_>,
) -> Result<String, OrchestratorError> {
    if parse_id(observed.full_database_id.as_deref()) != Some(repository_id)
        || observed.name_with_owner.as_deref() != Some(repository)
    {
        return Err(mismatch());
    }
    let pull = observed.pull_request.ok_or_else(mismatch)?;
    validate_pull(&pull, rest)?;
    let merge_sha = pull
        .merge_commit
        .and_then(|commit| commit.oid)
        .filter(|sha| valid_sha(sha))
        .ok_or_else(mismatch)?;
    if rest.merge_sha.is_some_and(|sha| sha != merge_sha) {
        return Err(mismatch());
    }
    Ok(merge_sha)
}

fn validate_pull(
    pull: &GraphqlPullRequest,
    rest: &RestLandedPull<'_>,
) -> Result<(), OrchestratorError> {
    let head = pull.head_repository.as_ref().ok_or_else(mismatch)?;
    if pull.number != Some(rest.number)
        || pull.state.as_deref() != Some("MERGED")
        || pull.merged != Some(true)
        || pull.merged_at.as_deref() != Some(rest.merged_at)
        || pull.head_ref_name.as_deref() != Some(rest.head.reference)
        || pull.head_ref_oid.as_deref() != Some(rest.head.sha)
        || pull.base_ref_name.as_deref() != Some(rest.base.reference)
        || pull.base_ref_oid.as_deref() != Some(rest.base.sha)
        || parse_id(head.full_database_id.as_deref()) != Some(rest.head.repository_id)
        || head.name_with_owner.as_deref() != Some(rest.head.repository)
    {
        return Err(mismatch());
    }
    Ok(())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_id(value: Option<&str>) -> Option<u64> {
    let value = value?;
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

fn mismatch() -> OrchestratorError {
    internal("tap_historical_merge_graphql_mismatch")
}

#[cfg(test)]
#[path = "github_landed_pull_tests.rs"]
mod tests;
