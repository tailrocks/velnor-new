//! Exact cache API census for one restore or save key.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::workflow::{
    BoundQualificationCacheKeys, QualificationCacheBackendEntry,
    QualificationCacheBackendObservation,
};
use velnor_actions_mise::{PinnedTool, PinnedToolExec};

const CACHE_PAGE_SIZE: u64 = 100;
const MAX_CACHE_CENSUS_PAGES: u64 = 256;
const MAX_CACHE_CENSUS_PAGES_USIZE: usize = 256;
const MAX_CACHE_CENSUS_PAGE_BYTES: usize = 1024 * 1024;
const MAX_CACHE_CENSUS_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_CACHE_REF_BYTES: usize = 4_096;
const CACHE_API_TIMEOUT: Duration = Duration::from_secs(90);

/// Query the pinned cache API and validate its complete result set.
///
/// # Errors
/// Returns an error when the workspace, repository, API request, or census is invalid.
pub fn observe_qualification_cache_backend(
    workspace: &Path,
    repository: &str,
    key: &str,
    git_ref: &str,
) -> Result<QualificationCacheBackendObservation, crate::OrchestratorError> {
    let repository = canonical_repository(repository)?;
    let mut census = QualificationCacheCensus::new(key, git_ref)
        .map_err(|_| crate::internal::internal("qualification_cache_census_filter"))?;
    let root = crate::root::resolve_root(workspace)?;
    let config = crate::config::load_config(&root)?;
    let catalog = crate::tool_catalog_resolve::resolve_tool_catalog(&root, &config)?;
    let request = cache_api_request(&repository, key, git_ref)?;
    let output = request
        .command(&catalog)
        .and_then(|command| command.run_bounded(MAX_CACHE_CENSUS_RESPONSE_BYTES, CACHE_API_TIMEOUT))
        .map_err(|_| crate::internal::internal("qualification_cache_api_unavailable"))?;
    if !output.success {
        return Err(crate::internal::internal(
            "qualification_cache_api_unavailable",
        ));
    }
    parse_slurped_census(&mut census, &output.stdout)
        .map_err(|_| crate::internal::internal("qualification_cache_census_invalid"))?;
    census
        .finish()
        .map_err(|_| crate::internal::internal("qualification_cache_census_incomplete"))
}

/// Admit cache-action outputs only when they agree with the typed key and a
/// complete backend census.
///
/// This records backend agreement. It does not admit restored payload bytes;
/// callers must compare the staged useful-state digest before import or use.
/// # Errors
/// Returns an error when the action output or backend observation differs from
/// the admitted restore expectation.
pub fn validate_qualification_cache_backend_observation(
    keys: &BoundQualificationCacheKeys,
    action_hit: bool,
    action_matched_key: Option<&str>,
    observed: &QualificationCacheBackendObservation,
) -> Result<(), crate::OrchestratorError> {
    let action_agrees = match &keys.restore {
        None => !action_hit && action_matched_key.is_none(),
        Some(expectation) => match &expectation.expected_cache {
            None => !action_hit && action_matched_key.is_none(),
            Some(_) => action_hit && action_matched_key == Some(expectation.requested_key.as_str()),
        },
    };
    if !action_agrees {
        return Err(crate::internal::internal(
            "qualification_cache_action_observation_mismatch",
        ));
    }
    keys.validate_restore_observation(action_matched_key, observed)
        .map_err(|_| crate::internal::internal("qualification_cache_backend_observation_mismatch"))
}

fn canonical_repository(value: &str) -> Result<String, crate::OrchestratorError> {
    let repository = crate::origin::validate_repository_slug(value)
        .ok_or_else(|| crate::internal::internal("qualification_cache_repository_invalid"))?;
    if !repository.split('/').all(|segment| {
        segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    }) {
        return Err(crate::internal::internal(
            "qualification_cache_repository_invalid",
        ));
    }
    Ok(repository)
}

fn cache_api_request(
    repository: &str,
    key: &str,
    git_ref: &str,
) -> Result<PinnedToolExec, crate::OrchestratorError> {
    QualificationCacheCensus::new(key, git_ref)
        .map_err(|_| crate::internal::internal("qualification_cache_census_filter"))?;
    let args = vec![
        OsString::from("api"),
        OsString::from(format!("repos/{repository}/actions/caches")),
        OsString::from("--paginate"),
        OsString::from("--slurp"),
        OsString::from("-f"),
        OsString::from(format!("key={key}")),
        OsString::from("-f"),
        OsString::from(format!("ref={git_ref}")),
        OsString::from("-F"),
        OsString::from("per_page=100"),
    ];
    PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| crate::internal::internal("qualification_cache_gh_unavailable"))
}

fn parse_slurped_census(
    census: &mut QualificationCacheCensus,
    bytes: &[u8],
) -> Result<(), &'static str> {
    let value = parse_strict_json_bytes(bytes, MAX_CACHE_CENSUS_RESPONSE_BYTES)
        .map_err(|_| "cache_census_response_invalid")?;
    let pages = value
        .as_array()
        .filter(|pages| !pages.is_empty() && pages.len() <= MAX_CACHE_CENSUS_PAGES_USIZE)
        .ok_or("cache_census_pages_invalid")?;
    for (index, page) in pages.iter().enumerate() {
        let page_number = u64::try_from(index + 1).map_err(|_| "cache_census_page_limit")?;
        let page_bytes = serde_json::to_vec(page).map_err(|_| "cache_census_page_json_invalid")?;
        census.push_page(page_number, &page_bytes)?;
    }
    Ok(())
}

/// Collect complete pages before returning exact backend evidence.
pub(crate) struct QualificationCacheCensus {
    expected_key: String,
    expected_ref: String,
    expected_total: Option<u64>,
    expected_pages: Option<u64>,
    next_page: u64,
    seen_entries: u64,
    ids: BTreeSet<u64>,
    exact_entry: Option<QualificationCacheBackendEntry>,
}

impl QualificationCacheCensus {
    /// Start a census for one exact cache key and Git ref.
    pub(crate) fn new(key: &str, git_ref: &str) -> Result<Self, &'static str> {
        if !valid_cache_key(key) || !valid_cache_ref(git_ref) {
            return Err("cache_census_filter_invalid");
        }
        Ok(Self {
            expected_key: key.to_owned(),
            expected_ref: git_ref.to_owned(),
            expected_total: None,
            expected_pages: None,
            next_page: 1,
            seen_entries: 0,
            ids: BTreeSet::new(),
            exact_entry: None,
        })
    }

    /// Add one ordered page from the cache API.
    pub(crate) fn push_page(&mut self, page_number: u64, bytes: &[u8]) -> Result<(), &'static str> {
        if page_number != self.next_page || page_number > MAX_CACHE_CENSUS_PAGES {
            return Err("cache_census_page_order_invalid");
        }
        let value = parse_strict_json_bytes(bytes, MAX_CACHE_CENSUS_PAGE_BYTES)
            .map_err(|_| "cache_census_page_json_invalid")?;
        let page: CachePage =
            serde_json::from_value(value).map_err(|_| "cache_census_page_shape_invalid")?;
        self.check_page_count(page_number, page.total_count, page.actions_caches.len())?;
        for record in page.actions_caches {
            let entry = record.into_entry()?;
            if !self.ids.insert(entry.id) {
                return Err("cache_census_duplicate_id");
            }
            if entry.key == self.expected_key && entry.git_ref == self.expected_ref {
                if self.exact_entry.replace(entry).is_some() {
                    return Err("cache_census_exact_match_ambiguous");
                }
            }
            self.seen_entries += 1;
        }
        self.next_page += 1;
        Ok(())
    }

    /// Finish only after every reported result has been checked.
    pub(crate) fn finish(self) -> Result<QualificationCacheBackendObservation, &'static str> {
        let expected_total = self.expected_total.ok_or("cache_census_page_missing")?;
        let expected_pages = self.expected_pages.ok_or("cache_census_page_missing")?;
        if self.next_page != expected_pages + 1 || self.seen_entries != expected_total {
            return Err("cache_census_incomplete");
        }
        Ok(self.exact_entry.map_or(
            QualificationCacheBackendObservation::Absent,
            QualificationCacheBackendObservation::Found,
        ))
    }

    fn check_page_count(
        &mut self,
        page_number: u64,
        total_count: u64,
        page_entries: usize,
    ) -> Result<(), &'static str> {
        if self
            .expected_total
            .is_some_and(|expected| expected != total_count)
        {
            return Err("cache_census_total_changed");
        }
        let page_count = total_count.div_ceil(CACHE_PAGE_SIZE).max(1);
        if page_count > MAX_CACHE_CENSUS_PAGES || page_number > page_count {
            return Err("cache_census_page_limit");
        }
        if self
            .expected_pages
            .is_some_and(|expected| expected != page_count)
        {
            return Err("cache_census_total_changed");
        }
        self.expected_total = Some(total_count);
        self.expected_pages = Some(page_count);
        let offset = (page_number - 1) * CACHE_PAGE_SIZE;
        let expected_entries = total_count.saturating_sub(offset).min(CACHE_PAGE_SIZE);
        if u64::try_from(page_entries).map_err(|_| "cache_census_page_size_invalid")?
            != expected_entries
        {
            return Err("cache_census_page_size_invalid");
        }
        Ok(())
    }
}

fn valid_cache_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'\\')
}

fn valid_cache_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CACHE_REF_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'\\')
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CachePage {
    total_count: u64,
    actions_caches: Vec<CacheRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheRecord {
    id: u64,
    key: String,
    #[serde(rename = "ref")]
    git_ref: String,
    version: String,
    size_in_bytes: u64,
    created_at: String,
    last_accessed_at: serde_json::Value,
}

impl CacheRecord {
    fn into_entry(self) -> Result<QualificationCacheBackendEntry, &'static str> {
        if self.id == 0
            || !valid_cache_key(&self.key)
            || !valid_cache_ref(&self.git_ref)
            || !valid_cache_version(&self.version)
            || self.created_at.is_empty()
            || !(self.last_accessed_at.is_null()
                || self
                    .last_accessed_at
                    .as_str()
                    .is_some_and(|value| !value.is_empty()))
        {
            return Err("cache_census_entry_invalid");
        }
        Ok(QualificationCacheBackendEntry {
            id: self.id,
            key: self.key,
            git_ref: self.git_ref,
            version: self.version,
            size_bytes: self.size_in_bytes,
        })
    }
}

fn valid_cache_version(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod request_tests {
    use super::{QualificationCacheCensus, cache_api_request, parse_slurped_census};
    use velnor_actions_mise::PinnedTool;

    #[test]
    fn api_request_uses_pinned_gh_and_keeps_query_values_separate() {
        let request = cache_api_request(
            "example/repository",
            "exact-key&suffix=value",
            "refs/heads/private/test",
        )
        .expect("request");
        assert_eq!(request.tools(), &[PinnedTool::Gh]);
        assert_eq!(request.program(), "gh");
        let args: Vec<_> = request
            .args()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "api",
                "repos/example/repository/actions/caches",
                "--paginate",
                "--slurp",
                "-f",
                "key=exact-key&suffix=value",
                "-f",
                "ref=refs/heads/private/test",
                "-F",
                "per_page=100",
            ]
        );
    }

    #[test]
    fn api_request_rejects_query_line_breaks() {
        assert!(cache_api_request("example/repository", "bad\nkey", "refs/heads/x").is_err());
    }

    #[test]
    fn slurped_pages_require_a_complete_census() {
        let mut census =
            QualificationCacheCensus::new("exact-key", "refs/heads/x").expect("census");
        let response = serde_json::json!([
            { "total_count": 1, "actions_caches": [] }
        ]);
        let bytes = serde_json::to_vec(&response).expect("response");
        assert!(parse_slurped_census(&mut census, &bytes).is_err());
    }
}

#[cfg(test)]
#[path = "qualification_cache_backend_tests.rs"]
mod tests;
