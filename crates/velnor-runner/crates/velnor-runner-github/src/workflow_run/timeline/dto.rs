// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BaseRefChangedEvent {
    pub id: String,

    pub database_id: Option<i64>,

    pub created_at: String,

    pub previous_ref_name: String,

    pub current_ref_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestBaseHistory {
    pub number: i64,

    pub repository_id: i64,

    pub head_repository_id: i64,

    pub head_ref: String,

    pub head_sha: String,

    pub base_repository_id: i64,

    pub base_ref: String,

    pub base_sha: String,

    pub updated_at: String,

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
