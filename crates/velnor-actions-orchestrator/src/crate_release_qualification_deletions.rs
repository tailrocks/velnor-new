// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    OrchestratorError,
    crate_release_qualification_provider::{ProviderQuery, ProviderRead},
    crate_release_qualification_source::ReviewedSourceRequest,
    internal::internal,
};
use std::collections::BTreeSet;

pub(crate) fn require_no_deleted_comments(
    candidate: &ReviewedSourceRequest,
    api: &dyn ProviderRead,
) -> Result<(), OrchestratorError> {
    let (owner, name) = candidate.repository.split_once('/').ok_or_else(invalid)?;
    let mut cursor: Option<String> = None;
    let mut cursors = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for _ in 0..100 {
        let mut fields = vec![
            ("owner", owner.to_owned()),
            ("name", name.to_owned()),
            ("number", candidate.pull_request.to_string()),
        ];
        if let Some(value) = &cursor {
            fields.push(("cursor", value.clone()));
        }
        let value = api.provider_graphql(ProviderQuery::DeletedComments, &fields)?;
        let repository = &value["data"]["repository"];
        if value.get("errors").is_some()
            || repository["nameWithOwner"].as_str() != Some(candidate.repository.as_str())
            || repository["fullDatabaseId"].as_str()
                != Some(candidate.repository_id.to_string().as_str())
            || repository
                .pointer("/pullRequest/number")
                .and_then(serde_json::Value::as_u64)
                != Some(candidate.pull_request)
        {
            return Err(invalid());
        }
        let page = &repository["pullRequest"]["timelineItems"];
        let nodes = page["nodes"]
            .as_array()
            .filter(|n| n.len() <= 100)
            .ok_or_else(invalid)?;
        for node in nodes {
            let id = node["id"]
                .as_str()
                .filter(|id| {
                    !id.is_empty() && id.len() <= 512 && id.bytes().all(|b| b.is_ascii_graphic())
                })
                .ok_or_else(invalid)?;
            if !ids.insert(id.to_owned())
                || node["createdAt"].as_str().is_none_or(str::is_empty)
                || ids.len() > 10_000
            {
                return Err(invalid());
            }
        }
        let more = page["pageInfo"]["hasNextPage"]
            .as_bool()
            .ok_or_else(invalid)?;
        let end = page["pageInfo"].get("endCursor").ok_or_else(invalid)?;
        if more {
            let next = end
                .as_str()
                .filter(|s| {
                    !s.is_empty() && s.len() <= 512 && s.bytes().all(|b| b.is_ascii_graphic())
                })
                .ok_or_else(invalid)?;
            if nodes.is_empty() || !cursors.insert(next.to_owned()) {
                return Err(invalid());
            }
            cursor = Some(next.to_owned());
        } else {
            if !(end.is_null()
                || end.as_str().is_some_and(|s| {
                    !s.is_empty() && s.len() <= 512 && s.bytes().all(|b| b.is_ascii_graphic())
                }))
            {
                return Err(invalid());
            }
            return if ids.is_empty() {
                Ok(())
            } else {
                Err(internal("qualification_deleted_feedback_unavailable"))
            };
        }
    }
    Err(invalid())
}

fn invalid() -> OrchestratorError {
    internal("qualification_deleted_feedback_invalid")
}
