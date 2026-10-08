//! Bounded Actions API inventory for artifact-build jobs and outputs.

use std::ffi::OsString;
use std::path::Path;

use serde::Deserialize;
use velnor_actions_contract::parse_strict_json;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator_cover::cover::shard::BaselineLookup;

const PER_PAGE: usize = 100;
const MAX_PAGES: usize = 100;
const MAX_ITEMS: usize = PER_PAGE * MAX_PAGES;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ApiJob {
    pub id: u64,
    pub run_id: u64,
    pub head_sha: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub name: String,
    pub runner_id: Option<u64>,
    pub runner_name: Option<String>,
    pub runner_group_id: Option<u64>,
    pub runner_group_name: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ApiArtifact {
    pub id: u64,
    pub name: String,
    pub size_in_bytes: u64,
    pub expired: bool,
    pub workflow_run: Option<ApiArtifactRun>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ApiArtifactRun {
    pub id: u64,
    pub repository_id: u64,
    pub head_sha: String,
}

#[derive(Debug, Deserialize)]
struct JobsPage {
    total_count: usize,
    jobs: Vec<ApiJob>,
}

#[derive(Debug, Deserialize)]
struct ArtifactsPage {
    total_count: usize,
    artifacts: Vec<ApiArtifact>,
}

/// List every job in one exact workflow-run attempt using bounded pages.
pub(super) fn list_jobs(
    catalog: &ToolCatalog,
    root: &Path,
    repo: &str,
    run_id: u64,
    attempt: u32,
) -> Result<Vec<ApiJob>, &'static str> {
    collect_pages(
        |page| {
            let endpoint = format!(
                "repos/{repo}/actions/runs/{run_id}/attempts/{attempt}/jobs?per_page={PER_PAGE}&page={page}"
            );
            let query = "{total_count, jobs: [.jobs[] | {id, run_id, head_sha, status, conclusion, name, runner_id, runner_name, runner_group_id, runner_group_name, labels}]}";
            let text = BaselineLookup::run(catalog, root, api_args(&endpoint, query))
                .map_err(|_| "actions_api_request_failed")?;
            let page = parse_page::<JobsPage>(&text)?;
            Ok((page.total_count, page.jobs))
        },
        |job: &ApiJob| job.id,
    )
}

/// List all artifacts associated with one run; names bind attempts later.
pub(super) fn list_artifacts(
    catalog: &ToolCatalog,
    root: &Path,
    repo: &str,
    run_id: u64,
) -> Result<Vec<ApiArtifact>, &'static str> {
    collect_pages(
        |page| {
            let endpoint = format!(
                "repos/{repo}/actions/runs/{run_id}/artifacts?per_page={PER_PAGE}&page={page}"
            );
            let query = "{total_count, artifacts: [.artifacts[] | {id, name, size_in_bytes, expired, workflow_run: (if .workflow_run == null then null else {id: .workflow_run.id, repository_id: .workflow_run.repository_id, head_sha: .workflow_run.head_sha} end)}]}";
            let text = BaselineLookup::run(catalog, root, api_args(&endpoint, query))
                .map_err(|_| "actions_api_request_failed")?;
            let page = parse_page::<ArtifactsPage>(&text)?;
            Ok((page.total_count, page.artifacts))
        },
        |artifact: &ApiArtifact| artifact.id,
    )
}

/// Exact CLI arguments; repository and numeric path components are validated first.
fn api_args(endpoint: &str, query: &str) -> Vec<OsString> {
    ["api", endpoint, "--jq", query]
        .iter()
        .map(OsString::from)
        .collect()
}

fn parse_page<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, &'static str> {
    let value = parse_strict_json(text).map_err(|_| "actions_api_malformed_json")?;
    serde_json::from_value(value).map_err(|_| "actions_api_malformed_page")
}

fn collect_pages<T>(
    mut read_page: impl FnMut(usize) -> Result<(usize, Vec<T>), &'static str>,
    id_of: impl Fn(&T) -> u64,
) -> Result<Vec<T>, &'static str> {
    let (total, first) = read_page(1)?;
    if total > MAX_ITEMS {
        return Err("actions_api_inventory_over_limit");
    }
    let page_count = total.div_ceil(PER_PAGE).max(1);
    if page_count > MAX_PAGES || first.len() != total.min(PER_PAGE) {
        return Err("actions_api_page_count_mismatch");
    }
    let mut all = Vec::with_capacity(total);
    all.extend(first);
    for page_number in 2..=page_count {
        let (page_total, page_items) = read_page(page_number)?;
        let consumed = (page_number - 1) * PER_PAGE;
        let expected = total.saturating_sub(consumed).min(PER_PAGE);
        if page_total != total || page_items.len() != expected {
            return Err("actions_api_page_count_mismatch");
        }
        all.extend(page_items);
    }
    let mut ids = std::collections::BTreeSet::new();
    if all.len() != total || all.iter().any(|item| !ids.insert(id_of(item))) {
        return Err("actions_api_duplicate_or_missing_id");
    }
    Ok(all)
}

#[cfg(test)]
#[path = "artifact_build_api/tests.rs"]
mod tests;
