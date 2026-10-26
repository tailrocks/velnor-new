use serde::Deserialize;
use serde_json::Value;

use crate::{OrchestratorError, internal::internal};

pub(crate) const QUERY: &str = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){databaseId nameWithOwner pullRequest(number:$number){number state merged mergedAt headRefName headRefOid baseRefName baseRefOid headRepository{databaseId nameWithOwner} mergeCommit{oid}}}}";

pub(crate) struct RestLandedPull<'a> {
    pub(crate) number: u64,
    pub(crate) state: &'a str,
    pub(crate) merged: bool,
    pub(crate) merged_at: &'a str,
    pub(crate) head_ref: &'a str,
    pub(crate) head_sha: &'a str,
    pub(crate) head_repository_id: u64,
    pub(crate) head_repository: &'a str,
    pub(crate) base_ref: &'a str,
    pub(crate) base_sha: &'a str,
    pub(crate) base_repository_id: u64,
    pub(crate) base_repository: &'a str,
}

pub(crate) fn repositories_match(
    repository_id: u64,
    repository: &str,
    head: Option<(u64, &str)>,
    base: Option<(u64, &str)>,
) -> bool {
    let matches = |observed: Option<(u64, &str)>| {
        observed.is_some_and(|(id, name)| id == repository_id && name == repository)
    };
    matches(head) && matches(base)
}

pub(crate) fn request_merge_oid(
    repository_id: u64,
    repository: &str,
    rest: &RestLandedPull<'_>,
    request: impl FnOnce(&str, &[(&str, String)]) -> Result<Value, OrchestratorError>,
) -> Result<String, OrchestratorError> {
    let (owner, name) = repository
        .split_once('/')
        .ok_or_else(|| internal("tap_historical_merge_graphql_mismatch"))?;
    let fields = [
        ("owner", owner.to_owned()),
        ("name", name.to_owned()),
        ("number", rest.number.to_string()),
    ];
    let response = request(QUERY, &fields)?;
    merge_oid(response, repository_id, repository, rest)
}

pub(crate) fn merge_oid(
    response: Value,
    repository_id: u64,
    repository: &str,
    rest: &RestLandedPull<'_>,
) -> Result<String, OrchestratorError> {
    if response
        .get("errors")
        .is_some_and(|errors| !errors.as_array().is_some_and(Vec::is_empty))
    {
        return Err(internal("tap_historical_merge_graphql_mismatch"));
    }
    let envelope: GraphqlEnvelope = serde_json::from_value(response)
        .map_err(|_| internal("tap_historical_merge_graphql_mismatch"))?;
    let Some(repository_data) = envelope.data.and_then(|data| data.repository) else {
        return Err(internal("tap_historical_merge_graphql_mismatch"));
    };
    let (Some(observed_repository_id), Some(observed_repository), Some(pull)) = (
        repository_data.database_id,
        repository_data.name_with_owner,
        repository_data.pull_request,
    ) else {
        return Err(internal("tap_historical_merge_graphql_mismatch"));
    };
    let (Some(head_repository_id), Some(head_repository), Some(merge_commit)) = (
        pull.head_repository
            .as_ref()
            .and_then(|head| head.database_id),
        pull.head_repository
            .as_ref()
            .and_then(|head| head.name_with_owner.as_deref()),
        pull.merge_commit,
    ) else {
        return Err(internal("tap_historical_merge_graphql_mismatch"));
    };
    let Some(merge_oid) = merge_commit.oid else {
        return Err(internal("tap_historical_merge_graphql_mismatch"));
    };
    if observed_repository_id != repository_id
        || observed_repository != repository
        || pull.number != Some(rest.number)
        || pull.state.as_deref() != Some("MERGED")
        || pull.merged != Some(true)
        || pull.merged_at.as_deref() != Some(rest.merged_at)
        || rest.state != "closed"
        || !rest.merged
        || pull.head_ref_name.as_deref() != Some(rest.head_ref)
        || pull.head_ref_oid.as_deref() != Some(rest.head_sha)
        || pull.base_ref_name.as_deref() != Some(rest.base_ref)
        || pull.base_ref_oid.as_deref() != Some(rest.base_sha)
        || head_repository_id != rest.head_repository_id
        || head_repository != rest.head_repository
        || rest.base_repository_id != repository_id
        || rest.base_repository != repository
        || !valid_sha(rest.head_sha)
        || !valid_sha(rest.base_sha)
        || !valid_sha(&merge_oid)
    {
        return Err(internal("tap_historical_merge_graphql_mismatch"));
    }
    Ok(merge_oid)
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Deserialize)]
struct GraphqlEnvelope {
    data: Option<GraphqlData>,
}

#[derive(Deserialize)]
struct GraphqlData {
    repository: Option<GraphqlRepository>,
}

#[derive(Deserialize)]
struct GraphqlRepository {
    #[serde(rename = "databaseId")]
    database_id: Option<u64>,
    #[serde(rename = "nameWithOwner")]
    name_with_owner: Option<String>,
    #[serde(rename = "pullRequest")]
    pull_request: Option<GraphqlPullRequest>,
}

#[derive(Deserialize)]
struct GraphqlPullRequest {
    number: Option<u64>,
    state: Option<String>,
    merged: Option<bool>,
    #[serde(rename = "mergedAt")]
    merged_at: Option<String>,
    #[serde(rename = "headRefName")]
    head_ref_name: Option<String>,
    #[serde(rename = "headRefOid")]
    head_ref_oid: Option<String>,
    #[serde(rename = "baseRefName")]
    base_ref_name: Option<String>,
    #[serde(rename = "baseRefOid")]
    base_ref_oid: Option<String>,
    #[serde(rename = "headRepository")]
    head_repository: Option<GraphqlHeadRepository>,
    #[serde(rename = "mergeCommit")]
    merge_commit: Option<GraphqlOid>,
}

#[derive(Deserialize)]
struct GraphqlHeadRepository {
    #[serde(rename = "databaseId")]
    database_id: Option<u64>,
    #[serde(rename = "nameWithOwner")]
    name_with_owner: Option<String>,
}

#[derive(Deserialize)]
struct GraphqlOid {
    oid: Option<String>,
}
