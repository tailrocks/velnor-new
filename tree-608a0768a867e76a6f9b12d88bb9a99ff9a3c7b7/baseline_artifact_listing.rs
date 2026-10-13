//! Complete exact-name listing for trusted baseline artifacts.

use std::collections::BTreeSet;
use std::ffi::OsString;

use crate::run_select::{BaselineArtifactMetadata, select_baseline_artifact_metadata};

const ARTIFACTS_PER_PAGE: u64 = 100;
const MAX_ARTIFACT_PAGES: u64 = 10;

/// Enumerate every exact-name result before accepting unique metadata.
pub(crate) fn select_metadata(
    repo: &str,
    run_id: u64,
    expected: &str,
    mut run: impl FnMut(Vec<OsString>) -> Result<String, String>,
) -> Result<BaselineArtifactMetadata, String> {
    let first = run(artifacts_page_args(repo, run_id, expected, 1)?)?;
    let (total_count, first_entries) = parse_page(&first)?;
    let page_count = total_count.div_ceil(ARTIFACTS_PER_PAGE);
    if page_count > MAX_ARTIFACT_PAGES {
        return Err("baseline_unavailable".to_owned());
    }
    validate_page_length(total_count, 1, first_entries.len())?;
    let mut seen_ids = BTreeSet::new();
    validate_service_ids(&first_entries, &mut seen_ids)?;
    let mut entries = first_entries;
    for page in 2..=page_count {
        let text = run(artifacts_page_args(repo, run_id, expected, page)?)?;
        let (page_total, page_entries) = parse_page(&text)?;
        if page_total != total_count {
            return Err("baseline_unavailable".to_owned());
        }
        validate_page_length(total_count, page, page_entries.len())?;
        validate_service_ids(&page_entries, &mut seen_ids)?;
        entries.extend(page_entries);
    }
    let complete = serde_json::json!({"total_count": total_count, "artifacts": entries});
    select_baseline_artifact_metadata(&complete.to_string(), expected)
}

/// Require positive, unique service IDs across every exact-name page.
fn validate_service_ids(
    entries: &[serde_json::Value],
    seen_ids: &mut BTreeSet<u64>,
) -> Result<(), String> {
    for entry in entries {
        let id = entry["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| "baseline_unavailable".to_owned())?;
        if !seen_ids.insert(id) {
            return Err("baseline_unavailable".to_owned());
        }
    }
    Ok(())
}

/// Build one bounded GitHub API page query for an exact baseline name.
pub(crate) fn artifacts_page_args(
    repo: &str,
    run_id: u64,
    expected: &str,
    page: u64,
) -> Result<Vec<OsString>, String> {
    if crate::origin::validate_repository_slug(repo).is_none()
        || run_id == 0
        || page == 0
        || page > MAX_ARTIFACT_PAGES
        || velnor_actions_contract::validate_artifact_id(expected).is_err()
    {
        return Err("baseline_unavailable".to_owned());
    }
    Ok(vec![
        OsString::from("api"),
        OsString::from(format!(
            "repos/{repo}/actions/runs/{run_id}/artifacts?name={expected}&per_page={ARTIFACTS_PER_PAGE}&page={page}"
        )),
    ])
}

/// Parse the service count and one page of artifact entries.
fn parse_page(text: &str) -> Result<(u64, Vec<serde_json::Value>), String> {
    let value = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let total_count = value["total_count"]
        .as_u64()
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    let entries = value["artifacts"]
        .as_array()
        .cloned()
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    Ok((total_count, entries))
}

/// Reject short pages that would make uniqueness unprovable.
fn validate_page_length(total_count: u64, page: u64, actual: usize) -> Result<(), String> {
    let offset = (page - 1)
        .checked_mul(ARTIFACTS_PER_PAGE)
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    let expected = total_count.saturating_sub(offset).min(ARTIFACTS_PER_PAGE);
    if u64::try_from(actual).ok() == Some(expected) {
        Ok(())
    } else {
        Err("baseline_unavailable".to_owned())
    }
}

#[cfg(test)]
#[path = "baseline_artifact_listing_tests.rs"]
mod tests;
