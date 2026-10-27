// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct GraphqlEnvelope {
    pub(super) data: Option<GraphqlData>,
}

#[derive(Deserialize)]
pub(super) struct GraphqlData {
    pub(super) repository: Option<GraphqlRepository>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GraphqlRepository {
    pub(super) full_database_id: Option<String>,
    pub(super) name_with_owner: Option<String>,
    pub(super) pull_request: Option<GraphqlPullRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GraphqlPullRequest {
    pub(super) number: Option<u64>,
    pub(super) state: Option<String>,
    pub(super) merged: Option<bool>,
    pub(super) merged_at: Option<String>,
    pub(super) head_ref_name: Option<String>,
    pub(super) head_ref_oid: Option<String>,
    pub(super) base_ref_name: Option<String>,
    pub(super) base_ref_oid: Option<String>,
    pub(super) head_repository: Option<GraphqlHeadRepository>,
    pub(super) merge_commit: Option<GraphqlCommit>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GraphqlHeadRepository {
    pub(super) full_database_id: Option<String>,
    pub(super) name_with_owner: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct GraphqlCommit {
    pub(super) oid: Option<String>,
}
