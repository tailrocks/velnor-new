// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::pagination;

pub(super) fn api_run_data(
    route: &str,
    state: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let path = route.split('?').next().ok_or("invalid route")?;
    let run = &state["producer"];
    let run_id = run["id"].as_u64().ok_or("invalid producer run state")?;
    let requested_attempt = state["requested_attempt"]
        .as_u64()
        .ok_or("invalid requested producer run attempt")?;
    if let Some(run) = identified_run_data(route, path, state)? {
        return Ok(run);
    }
    if path.ends_with(&format!(
        "actions/runs/{run_id}/attempts/{requested_attempt}"
    )) {
        return Ok(run.clone());
    }
    if path.ends_with(&format!("actions/runs/{run_id}/artifacts")) {
        let artifacts = state["artifacts"]
            .as_array()
            .ok_or("missing artifact list")?;
        return pagination::page(
            route,
            state,
            "artifact_pages",
            serde_json::json!({"total_count":artifacts.len(),"artifacts":artifacts}),
        );
    }
    if path.contains("/actions/runs/") && path.ends_with("/jobs") {
        return ci_job_page(route, path, state);
    }
    if path.contains("actions/workflows/ci.yml/runs") {
        let query = route
            .split_once('?')
            .map(|(_, query)| query)
            .map_or("", |query| query);
        let fields = query.split('&').collect::<Vec<_>>();
        let nonces = fields
            .iter()
            .filter_map(|field| field.strip_prefix("_velnor_read_nonce="))
            .collect::<Vec<_>>();
        let authority_query = fields
            .iter()
            .filter(|field| !field.starts_with("_velnor_read_nonce="))
            .copied()
            .collect::<Vec<_>>()
            .join("&");
        let page = pagination::page_number(route)?;
        let expected = format!(
            "branch=main&event=push&head_sha={}&per_page=100&page={page}",
            state["source_commit"]
                .as_str()
                .ok_or("missing selected source")?
        );
        let first_page = format!(
            "branch=main&event=push&head_sha={}&per_page=100",
            state["source_commit"]
                .as_str()
                .ok_or("missing selected source")?
        );
        if (authority_query != expected && !(page == 1 && authority_query == first_page))
            || nonces.len() != 1
            || nonces[0].is_empty()
        {
            return Err("CI run query did not match selected source".into());
        }
        let runs = state["ci_runs"]
            .as_array()
            .map_or_else(|| vec![state["ci_run"].clone()], Clone::clone);
        return pagination::page(
            route,
            state,
            "ci_run_pages",
            serde_json::json!({"total_count":runs.len(),"workflow_runs":runs}),
        );
    }
    Err(format!("unhandled GitHub API route: {path}").into())
}

fn identified_run_data(
    route: &str,
    path: &str,
    state: &serde_json::Value,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error>> {
    let publisher = &state["publisher"];
    let publisher_id = publisher["id"]
        .as_u64()
        .ok_or("invalid publisher run state")?;
    let publisher_attempt = publisher["run_attempt"]
        .as_u64()
        .ok_or("invalid publisher attempt")?;
    if path.ends_with(&format!(
        "actions/runs/{publisher_id}/attempts/{publisher_attempt}"
    )) {
        return Ok(Some(publisher.clone()));
    }
    let attestor = &state["attestor"];
    let attestor_id = attestor["id"]
        .as_u64()
        .ok_or("invalid attestor run state")?;
    let attestor_attempt = attestor["run_attempt"]
        .as_u64()
        .ok_or("invalid attestor attempt")?;
    if attestor_id > 0 {
        if path.ends_with(&format!(
            "actions/runs/{attestor_id}/attempts/{attestor_attempt}"
        )) {
            return Ok(Some(attestor.clone()));
        }
        if path.ends_with(&format!(
            "actions/runs/{attestor_id}/attempts/{attestor_attempt}/jobs"
        )) {
            let jobs = state["jobs"].as_array().ok_or("missing attestor jobs")?;
            return pagination::page(
                route,
                state,
                "attestor_job_pages",
                serde_json::json!({"total_count":jobs.len(),"jobs":jobs}),
            )
            .map(Some);
        }
    }
    let qualifier = &state["qualifier"];
    let qualifier_id = qualifier["id"]
        .as_u64()
        .ok_or("invalid qualifier run state")?;
    let qualifier_attempt = qualifier["run_attempt"]
        .as_u64()
        .ok_or("invalid qualifier attempt")?;
    if path.ends_with(&format!(
        "actions/runs/{qualifier_id}/attempts/{qualifier_attempt}"
    )) {
        let mut response = qualifier.clone();
        if let Some(attempt) = state.get("qualifier_response_run_attempt") {
            response["run_attempt"] = attempt.clone();
        }
        return Ok(Some(response));
    }
    if path.ends_with(&format!(
        "actions/runs/{qualifier_id}/attempts/{qualifier_attempt}/jobs"
    )) {
        let jobs = state["qualifier_jobs"]
            .as_array()
            .ok_or("missing qualifier jobs")?;
        return pagination::page(
            route,
            state,
            "qualifier_job_pages",
            serde_json::json!({"total_count":jobs.len(),"jobs":jobs}),
        )
        .map(Some);
    }
    Ok(None)
}

fn ci_job_page(
    route: &str,
    path: &str,
    state: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let requested = path
        .strip_prefix("repos/tailrocks/velnor-new/actions/runs/")
        .and_then(|path| path.strip_suffix("/jobs"))
        .ok_or("invalid CI job route")?;
    let (run_id, attempt) = requested
        .split_once("/attempts/")
        .ok_or("CI job route has no attempt")?;
    let run_id = run_id.parse::<u64>()?;
    let attempt = attempt.parse::<u32>()?;
    let selected_run = state["ci_runs"]
        .as_array()
        .and_then(|runs| {
            runs.iter().find(|run| {
                run["id"].as_u64() == Some(run_id)
                    && run["run_attempt"].as_u64() == Some(u64::from(attempt))
            })
        })
        .unwrap_or(&state["ci_run"]);
    let matches_run = selected_run["id"].as_u64() == Some(run_id)
        && selected_run["run_attempt"].as_u64() == Some(u64::from(attempt));
    let query = route
        .split_once('?')
        .map(|(_, query)| query)
        .ok_or("CI job route has no query")?;
    let fields = query.split('&').collect::<Vec<_>>();
    let nonces = fields
        .iter()
        .filter_map(|field| field.strip_prefix("_velnor_read_nonce="))
        .collect::<Vec<_>>();
    let authority_query = fields
        .iter()
        .filter(|field| !field.starts_with("_velnor_read_nonce="))
        .copied()
        .collect::<Vec<_>>()
        .join("&");
    let page = fields
        .iter()
        .find_map(|field| field.strip_prefix("page="))
        .and_then(|page| page.parse::<usize>().ok())
        .ok_or("CI job route has no page")?;
    if !matches_run
        || page == 0
        || authority_query != format!("per_page=100&page={page}")
        || nonces.len() != 1
        || nonces[0].is_empty()
    {
        return Err("CI job query did not match the selected run attempt".into());
    }
    if let Some(pages) = state["ci_job_pages"].as_array() {
        return pages
            .get(page - 1)
            .cloned()
            .ok_or_else(|| "unexpected CI jobs page".into());
    }
    let jobs = state["ci_jobs"]
        .as_array()
        .ok_or("CI job inventory is missing")?;
    Ok(serde_json::json!({"total_count":jobs.len(),"jobs":jobs}))
}
