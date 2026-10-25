//! GraphQL records and verified timeline values.

use serde::{Deserialize, Serialize};

/// One authenticated base-ref change from the GitHub timeline.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BaseRefChangedEvent {
    /// Stable GraphQL node identity.
    pub id: String,
    /// Database identity when GitHub returns it.
    pub database_id: Option<i64>,
    /// API timestamp for ordering. Ties fail closed.
    pub created_at: String,
    /// Base ref before the event.
    pub previous_ref_name: String,
    /// Base ref after the event.
    pub current_ref_name: String,
}

/// Current pull request snapshot and its full base-change timeline.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestBaseHistory {
    /// Pull request number.
    pub number: i64,
    /// Immutable base repository ID.
    pub repository_id: i64,
    /// Immutable head repository ID.
    pub head_repository_id: i64,
    /// Current head ref.
    pub head_ref: String,
    /// Current head commit SHA.
    pub head_sha: String,
    /// Immutable base repository ID from the base ref.
    pub base_repository_id: i64,
    /// Current base ref.
    pub base_ref: String,
    /// Current base commit SHA.
    pub base_sha: String,
    /// Pull request update time from the same API snapshot.
    pub updated_at: String,
    /// Every base-ref change returned by the completed pagination walk.
    pub events: Vec<BaseRefChangedEvent>,
}

#[derive(Serialize)]
pub(super) struct GraphQlRequest<'a> {
    pub(super) query: &'static str,
    pub(super) variables: Variables<'a>,
}

#[derive(Serialize)]
pub(super) struct Variables<'a> {
    pub(super) owner: &'a str,
    pub(super) name: &'a str,
    pub(super) number: i64,
    pub(super) after: Option<&'a str>,
}

#[derive(Deserialize)]
pub(super) struct GraphQlResponse {
    pub(super) data: Option<Data>,
    #[serde(default)]
    pub(super) errors: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
pub(super) struct Data {
    pub(super) repository: Option<RepositoryNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RepositoryNode {
    pub(super) database_id: Option<i64>,
    pub(super) name: String,
    pub(super) owner: Owner,
    pub(super) pull_request: Option<PullRequestNode>,
}

#[derive(Deserialize)]
pub(super) struct Owner {
    pub(super) login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PullRequestNode {
    pub(super) number: i64,
    pub(super) updated_at: String,
    pub(super) head_ref_name: String,
    pub(super) head_ref_oid: String,
    pub(super) head_repository: Option<BranchRepository>,
    pub(super) base_ref_name: String,
    pub(super) base_ref_oid: String,
    pub(super) base_repository: Option<BranchRepository>,
    pub(super) timeline_items: TimelineConnection,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BranchRepository {
    pub(super) database_id: Option<i64>,
    pub(super) name: String,
    pub(super) owner: Owner,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TimelineConnection {
    pub(super) nodes: Vec<TimelineNode>,
    pub(super) page_info: PageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PageInfo {
    pub(super) has_next_page: bool,
    pub(super) end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TimelineNode {
    #[serde(rename = "__typename")]
    pub(super) kind: String,
    pub(super) id: String,
    pub(super) database_id: Option<i64>,
    pub(super) created_at: String,
    pub(super) previous_ref_name: String,
    pub(super) current_ref_name: String,
    pub(super) pull_request: TimelinePullRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TimelinePullRequest {
    pub(super) number: i64,
    pub(super) repository: Option<BranchRepository>,
}

pub(super) struct Snapshot {
    pub(super) history: PullRequestBaseHistory,
    pub(super) has_next_page: bool,
    pub(super) end_cursor: Option<String>,
}
