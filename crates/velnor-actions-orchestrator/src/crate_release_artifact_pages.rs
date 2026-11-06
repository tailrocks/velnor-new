// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_actions_mise::CheckDeadline;

use crate::crate_release_github::GitHubAdmissionRead;
use crate::{OrchestratorError, internal::internal};

const MAX_ARTIFACTS: usize = 10_000;
const PAGE_SIZE: usize = 100;
const MAX_BYTES: usize = 32 * 1024 * 1024;

pub(crate) fn list_artifacts<G: GitHubAdmissionRead + ?Sized>(
    api: &G,
    prefix: &str,
    run_id: u64,
) -> Result<Value, OrchestratorError> {
    let invalid = || internal("crate_release_artifact_pages_invalid");
    let deadline = CheckDeadline::after(Duration::from_secs(180)).map_err(|_| invalid())?;
    deadline
        .scope(|| {
            let mut remaining = MAX_BYTES;
            let first = list_pass(api, prefix, run_id, &mut remaining)?;
            if first.len() > PAGE_SIZE && first != list_pass(api, prefix, run_id, &mut remaining)? {
                return Err(invalid());
            }
            Ok(json!({"total_count":first.len(),"artifacts":first}))
        })
        .map_err(|_| invalid())?
}

fn list_pass<G: GitHubAdmissionRead + ?Sized>(
    api: &G,
    prefix: &str,
    run_id: u64,
    remaining: &mut usize,
) -> Result<Vec<Value>, OrchestratorError> {
    let invalid = || internal("crate_release_artifact_pages_invalid");
    if run_id == 0 {
        return Err(invalid());
    }
    let route = format!("{prefix}/actions/runs/{run_id}/artifacts?per_page=100");
    let mut output = Vec::new();
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut total = None;
    for page in 1..=MAX_ARTIFACTS / PAGE_SIZE {
        let page_route = if page == 1 {
            route.clone()
        } else {
            format!("{route}&page={page}")
        };
        let (response, bytes) = api.get_artifact_page(
            &page_route,
            u64::try_from(*remaining).map_err(|_| invalid())?,
        )?;
        let bytes = usize::try_from(bytes).map_err(|_| invalid())?;
        if bytes == 0 {
            return Err(invalid());
        }
        *remaining = remaining.checked_sub(bytes).ok_or_else(invalid)?;
        let count = response["total_count"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n <= MAX_ARTIFACTS)
            .ok_or_else(invalid)?;
        if total.is_some_and(|expected| expected != count) {
            return Err(invalid());
        }
        total = Some(count);
        let entries = response["artifacts"].as_array().ok_or_else(invalid)?;
        if entries.len() != (count.saturating_sub(output.len())).min(PAGE_SIZE) {
            return Err(invalid());
        }
        for entry in entries {
            let id = entry["id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(invalid)?;
            let name = entry["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or_else(invalid)?;
            if !ids.insert(id) || !names.insert(name.to_owned()) {
                return Err(invalid());
            }
            output.push(entry.clone());
        }
        if output.len() == count {
            return Ok(output);
        }
    }
    Err(invalid())
}
