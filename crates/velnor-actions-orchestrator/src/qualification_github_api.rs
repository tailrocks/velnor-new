//! Read-only GitHub API queries for hosted qualification provenance.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
pub(crate) use velnor_actions_contract::workflow::QUALIFICATION_CACHE_RECEIPT_ARTIFACT as RECEIPT_ARTIFACT_NAME;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use crate::OrchestratorError;
use crate::internal::internal;

/// Maximum bounded GitHub API JSON response bytes.
const MAX_API_JSON_BYTES: usize = 1 << 20;
/// Maximum downloaded immutable receipt ZIP bytes.
const MAX_API_ARCHIVE_BYTES: usize =
    crate::qualification_receipt_archive::MAX_RECEIPT_ARCHIVE_BYTES;
/// Bound each API invocation, including redirecting artifact archive calls.
const API_TIMEOUT: Duration = Duration::from_secs(60);

/// GitHub API scope fixed to one validated repository and checkout root.
#[derive(Debug, Clone)]
pub(crate) struct QualificationGitHubApi {
    repo: String,
    root: PathBuf,
    catalog: ToolCatalog,
}

/// Repository identity and default branch returned by GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepositoryApiRecord {
    pub(crate) full_name: String,
    pub(crate) default_branch: String,
}

/// Exact workflow-run attempt returned by GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunApiRecord {
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) path_ref: String,
    pub(crate) event: String,
    pub(crate) status: String,
    pub(crate) conclusion: Option<String>,
    pub(crate) head_branch: String,
    pub(crate) head_sha: String,
    pub(crate) repository: String,
}

/// Immutable artifact metadata returned by the Actions API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtifactApiRecord {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) digest: String,
    pub(crate) size_bytes: u64,
    pub(crate) expired: bool,
    pub(crate) workflow_run_id: u64,
    pub(crate) workflow_head_branch: String,
    pub(crate) workflow_head_sha: String,
}

impl QualificationGitHubApi {
    /// Create a scoped reader for one canonical repository.
    pub(crate) fn new(repo: &str, root: &Path) -> Result<Self, OrchestratorError> {
        let Some(repo) = crate::origin::validate_repository_slug(repo) else {
            return Err(internal("qualification_repository_invalid"));
        };
        Ok(Self {
            repo,
            root: root.to_path_buf(),
            catalog: ToolCatalog::pinned(),
        })
    }

    /// Read repository canonical name and configured default branch.
    pub(crate) fn repository(&self) -> Result<RepositoryApiRecord, OrchestratorError> {
        let value = self.json(&format!("repos/{}", self.repo))?;
        let record = RepositoryApiRecord {
            full_name: field_string(&value, "full_name")?,
            default_branch: field_string(&value, "default_branch")?,
        };
        if record.full_name != self.repo || !valid_branch_name(&record.default_branch) {
            return Err(internal("qualification_repository_metadata_mismatch"));
        }
        Ok(record)
    }

    /// Verify GitHub currently marks the exact default branch protected.
    pub(crate) fn branch_is_protected(&self, branch: &str) -> Result<bool, OrchestratorError> {
        let segment = encode_path_segment(branch);
        let value = self.json(&format!("repos/{}/branches/{segment}", self.repo))?;
        value["protected"]
            .as_bool()
            .ok_or_else(|| internal("qualification_branch_protection_unavailable"))
    }

    /// Read one exact workflow run attempt, never the mutable latest attempt.
    pub(crate) fn run_attempt(
        &self,
        run_id: u64,
        attempt: u32,
        default_branch: &str,
    ) -> Result<RunApiRecord, OrchestratorError> {
        if run_id == 0 || attempt == 0 {
            return Err(internal("qualification_run_reference_invalid"));
        }
        let route = format!(
            "repos/{}/actions/runs/{run_id}/attempts/{attempt}",
            self.repo
        );
        let value = self.json(&route)?;
        run_record(&value, run_id, attempt, default_branch)
    }

    /// Select one exact named artifact from the bounded run artifact listing.
    pub(crate) fn receipt_artifact(
        &self,
        run_id: u64,
        artifact_name: &str,
    ) -> Result<ArtifactApiRecord, OrchestratorError> {
        let route = format!(
            "repos/{}/actions/runs/{run_id}/artifacts?per_page=100",
            self.repo
        );
        let value = self.json(&route)?;
        let total = value["total_count"]
            .as_u64()
            .ok_or_else(|| internal("qualification_artifact_listing_invalid"))?;
        let entries = value["artifacts"]
            .as_array()
            .ok_or_else(|| internal("qualification_artifact_listing_invalid"))?;
        if total > 100 || !u64::try_from(entries.len()).is_ok_and(|length| length == total) {
            return Err(internal("qualification_artifact_listing_incomplete"));
        }
        let mut matches = entries
            .iter()
            .filter(|entry| entry["name"].as_str() == Some(artifact_name));
        let Some(entry) = matches.next() else {
            return Err(internal("qualification_receipt_artifact_missing"));
        };
        if matches.next().is_some() {
            return Err(internal("qualification_receipt_artifact_ambiguous"));
        }
        artifact_record(entry, run_id)
    }

    /// Confirm GitHub considers the exact base an ancestor of the source.
    pub(crate) fn base_is_ancestor(
        &self,
        base: &str,
        source: &str,
    ) -> Result<bool, OrchestratorError> {
        if !is_lower_sha(base) || !is_lower_sha(source) || base == source {
            return Err(internal("qualification_source_range_invalid"));
        }
        let route = format!("repos/{}/compare/{base}...{source}?per_page=1", self.repo);
        let value = self.json(&route)?;
        let status = field_string(&value, "status")?;
        let ahead = value["ahead_by"]
            .as_u64()
            .ok_or_else(|| internal("qualification_source_compare_invalid"))?;
        Ok(status == "ahead" && ahead > 0)
    }

    /// Download the exact API artifact ID as bounded raw ZIP bytes.
    pub(crate) fn download_artifact(&self, artifact_id: u64) -> Result<Vec<u8>, OrchestratorError> {
        if artifact_id == 0 {
            return Err(internal("qualification_artifact_id_invalid"));
        }
        let route = format!("repos/{}/actions/artifacts/{artifact_id}/zip", self.repo);
        let output = self.run_gh(
            vec![
                OsString::from("api"),
                OsString::from(route),
                OsString::from("-H"),
                OsString::from("Accept: application/vnd.github+json"),
            ],
            MAX_API_ARCHIVE_BYTES,
        )?;
        if !output.success || output.stdout.is_empty() {
            return Err(internal("qualification_artifact_download_failed"));
        }
        Ok(output.stdout)
    }

    /// Read one bounded JSON endpoint response.
    fn json(&self, route: &str) -> Result<Value, OrchestratorError> {
        let output = self.run_gh(
            vec![OsString::from("api"), OsString::from(route)],
            MAX_API_JSON_BYTES,
        )?;
        if !output.success {
            return Err(internal("qualification_api_request_failed"));
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|_| internal("qualification_api_response_invalid"))
    }

    fn run_gh(
        &self,
        args: Vec<OsString>,
        output_cap: usize,
    ) -> Result<velnor_actions_mise::ProcessOutput, OrchestratorError> {
        let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
            .map_err(|_| internal("qualification_gh_command_invalid"))?;
        exec.command(&self.catalog)
            .map_err(|_| internal("qualification_gh_command_invalid"))?
            .with_cwd(self.root.clone())
            .run_bounded(output_cap, API_TIMEOUT)
            .map_err(|_| internal("qualification_gh_command_failed"))
    }
}

fn run_record(
    value: &Value,
    expected_id: u64,
    expected_attempt: u32,
    default_branch: &str,
) -> Result<RunApiRecord, OrchestratorError> {
    let run_id = field_u64(value, "id")?;
    let run_attempt = field_u64(value, "run_attempt")?;
    let head_branch = field_string(value, "head_branch")?;
    let head_sha = field_string(value, "head_sha")?;
    let repository = field_string(&value["repository"], "full_name")?;
    let path_ref = normalize_workflow_path(&field_string(value, "path")?, default_branch)?;
    if run_id != expected_id
        || run_attempt != u64::from(expected_attempt)
        || head_branch != default_branch
        || !is_lower_sha(&head_sha)
    {
        return Err(internal("qualification_run_metadata_mismatch"));
    }
    Ok(RunApiRecord {
        run_id,
        run_attempt: expected_attempt,
        path_ref,
        event: field_string(value, "event")?,
        status: field_string(value, "status")?,
        conclusion: value["conclusion"].as_str().map(str::to_owned),
        head_branch,
        head_sha,
        repository,
    })
}

fn artifact_record(value: &Value, run_id: u64) -> Result<ArtifactApiRecord, OrchestratorError> {
    let record = ArtifactApiRecord {
        id: field_u64(value, "id")?,
        name: field_string(value, "name")?,
        digest: field_string(value, "digest")?,
        size_bytes: field_u64(value, "size_in_bytes")?,
        expired: value["expired"]
            .as_bool()
            .ok_or_else(|| internal("qualification_artifact_metadata_invalid"))?,
        workflow_run_id: field_u64(&value["workflow_run"], "id")?,
        workflow_head_branch: field_string(&value["workflow_run"], "head_branch")?,
        workflow_head_sha: field_string(&value["workflow_run"], "head_sha")?,
    };
    if record.id == 0
        || record.workflow_run_id != run_id
        || record.expired
        || record.size_bytes == 0
        || !usize::try_from(record.size_bytes).is_ok_and(|size| size <= MAX_API_ARCHIVE_BYTES)
        || !is_sha256_digest(&record.digest)
        || !is_lower_sha(&record.workflow_head_sha)
    {
        return Err(internal("qualification_artifact_metadata_mismatch"));
    }
    Ok(record)
}

fn normalize_workflow_path(path: &str, branch: &str) -> Result<String, OrchestratorError> {
    let (workflow, reported_ref) = path
        .split_once('@')
        .map_or((path, None), |(a, b)| (a, Some(b)));
    let valid_ref = reported_ref
        .is_none_or(|reported| reported == branch || reported == format!("refs/heads/{branch}"));
    if workflow != ".github/workflows/ci.yml" || !valid_ref {
        return Err(internal("qualification_workflow_path_mismatch"));
    }
    Ok(format!("{workflow}@{branch}"))
}

fn field_string(value: &Value, field: &str) -> Result<String, OrchestratorError> {
    value[field]
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| internal("qualification_api_field_missing"))
}

fn field_u64(value: &Value, field: &str) -> Result<u64, OrchestratorError> {
    value[field]
        .as_u64()
        .ok_or_else(|| internal("qualification_api_field_missing"))
}

fn is_lower_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| is_lower_hex(digest, 64))
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_branch_name(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with('/')
        && !branch.ends_with('/')
        && !branch.starts_with('.')
        && !branch.ends_with('.')
        && !branch.contains("..")
        && !branch.contains("@{")
        && branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
}

fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
#[path = "qualification_github_api_tests.rs"]
mod tests;
