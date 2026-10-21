//! Read bounded API pages for cache entries on one Git ref.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::workflow::QualificationCacheBackendEntry;
use velnor_actions_mise::{PinnedTool, PinnedToolExec};

use super::CachePage;

const PAGE_SIZE: u64 = 100;
const MAX_PAGES: u64 = 256;
const PAGE_BYTES: usize = 1024 * 1024;
const RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const API_TIMEOUT: Duration = Duration::from_secs(90);

pub(super) fn query_ref_census(
    workspace: &Path,
    repository: &str,
    git_ref: &str,
) -> Result<Vec<QualificationCacheBackendEntry>, crate::OrchestratorError> {
    let repository = super::super::canonical_repository(repository)?;
    if git_ref.is_empty()
        || git_ref.len() > 4_096
        || !git_ref
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'\\')
    {
        return Err(crate::internal::internal(
            "qualification_cache_census_filter",
        ));
    }
    let root = crate::root::resolve_root(workspace)?;
    let config = crate::config::load_config(&root)?;
    let catalog = crate::tool_catalog_resolve::resolve_tool_catalog(&root, &config)?;
    let request = cache_ref_api_request(&repository, git_ref)?;
    let output = request
        .command(&catalog)
        .and_then(|command| command.run_bounded(RESPONSE_BYTES, API_TIMEOUT))
        .map_err(|_| crate::internal::internal("qualification_cache_api_unavailable"))?;
    if !output.success {
        return Err(crate::internal::internal(
            "qualification_cache_api_unavailable",
        ));
    }
    parse_ref_pages(&output.stdout, git_ref)
        .map_err(|_| crate::internal::internal("qualification_cache_census_incomplete"))
}

pub(super) fn cache_ref_api_request(
    repository: &str,
    git_ref: &str,
) -> Result<PinnedToolExec, crate::OrchestratorError> {
    let args = vec![
        OsString::from("api"),
        OsString::from("--method"),
        OsString::from("GET"),
        OsString::from(format!("repos/{repository}/actions/caches")),
        OsString::from("--paginate"),
        OsString::from("--slurp"),
        OsString::from("-f"),
        OsString::from(format!("ref={git_ref}")),
        OsString::from("-F"),
        OsString::from("per_page=100"),
    ];
    PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| crate::internal::internal("qualification_cache_gh_unavailable"))
}

pub(super) fn parse_ref_pages(
    bytes: &[u8],
    expected_ref: &str,
) -> Result<Vec<QualificationCacheBackendEntry>, &'static str> {
    let value = parse_strict_json_bytes(bytes, RESPONSE_BYTES)
        .map_err(|_| "cache_census_response_invalid")?;
    let pages = value
        .as_array()
        .filter(|pages| {
            !pages.is_empty() && u64::try_from(pages.len()).is_ok_and(|count| count <= MAX_PAGES)
        })
        .ok_or("cache_census_pages_invalid")?;
    let mut entries = Vec::new();
    let mut expected_total = None;
    let mut expected_page_count = None;
    let mut ids = BTreeSet::new();
    for (index, page_value) in pages.iter().enumerate() {
        let page_number = u64::try_from(index + 1).map_err(|_| "cache_census_page_limit")?;
        let page_bytes =
            serde_json::to_vec(page_value).map_err(|_| "cache_census_page_json_invalid")?;
        if page_bytes.len() > PAGE_BYTES {
            return Err("cache_census_page_json_invalid");
        }
        let page: CachePage =
            serde_json::from_slice(&page_bytes).map_err(|_| "cache_census_page_shape_invalid")?;
        if expected_total.is_some_and(|total| total != page.total_count) {
            return Err("cache_census_total_changed");
        }
        let page_count = page.total_count.div_ceil(PAGE_SIZE).max(1);
        if page_count > MAX_PAGES
            || expected_page_count.is_some_and(|count| count != page_count)
            || page_number > page_count
        {
            return Err("cache_census_page_limit");
        }
        let offset = (page_number - 1) * PAGE_SIZE;
        let expected_entries = page.total_count.saturating_sub(offset).min(PAGE_SIZE);
        if u64::try_from(page.actions_caches.len()).map_err(|_| "cache_census_page_size_invalid")?
            != expected_entries
        {
            return Err("cache_census_page_size_invalid");
        }
        expected_total = Some(page.total_count);
        expected_page_count = Some(page_count);
        for record in page.actions_caches {
            let entry = record.into_entry()?;
            if entry.git_ref != expected_ref || !ids.insert(entry.id) {
                return Err("cache_census_entry_invalid");
            }
            entries.push(entry);
        }
    }
    if expected_page_count
        != Some(u64::try_from(pages.len()).map_err(|_| "cache_census_page_limit")?)
        || expected_total
            != Some(u64::try_from(entries.len()).map_err(|_| "cache_census_incomplete")?)
    {
        return Err("cache_census_incomplete");
    }
    super::sort_entries(&mut entries);
    Ok(entries)
}

pub(super) fn unix_ms() -> Result<u64, crate::OrchestratorError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| crate::internal::internal("qualification_cache_clock_invalid"))?
        .as_millis();
    u64::try_from(millis)
        .map_err(|_| crate::internal::internal("qualification_cache_clock_invalid"))
}
