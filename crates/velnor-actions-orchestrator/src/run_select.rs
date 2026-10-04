//! Exact-base run and artifact selection for trusted baseline lookup.

/// Successful exact-base run selected from service summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedBaseRun {
    /// Numeric GitHub run ID.
    pub run_id: u64,
    /// Successful current attempt selected from the run summary.
    pub attempt: u64,
}

/// Immutable service receipt for one exact-name baseline artifact.
///
/// The artifact name binds only source commit and compatibility. Run identity
/// comes from the selected run and service metadata; attempt identity comes
/// from the selected/API attempt and the manifest proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BaselineArtifactReceipt {
    /// GitHub's immutable artifact ID, used for the ZIP download endpoint.
    pub(crate) service_id: u64,
    /// SHA-256 advertised by the service, without its `sha256:` prefix.
    pub(crate) sha256: String,
    /// Advertised compressed ZIP byte count.
    pub(crate) size_bytes: u64,
    /// Exact two-argument baseline artifact name.
    pub(crate) name: String,
    /// Plan compatibility encoded in the immutable name.
    pub(crate) compatibility_id: String,
    /// Workflow run ID from the selected run and service metadata.
    pub(crate) run_id: u64,
    /// Source commit from the exact run and immutable name.
    pub(crate) head_sha: String,
    /// Protected branch from the exact run metadata.
    pub(crate) head_branch: String,
}

/// Select the newest successful push run for the exact source and branch.
///
/// The successful summary selects the run and bounds the allowed manifest
/// attempt. The exact attempt API binds the manifest attempt before coverage
/// is trusted.
/// # Errors
/// Refuses malformed listings, missing attempt data, and absent exact runs.
pub fn select_exact_base_run(
    text: &str,
    base: &str,
    branch: &str,
) -> Result<SelectedBaseRun, String> {
    let value = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let runs = value
        .as_array()
        .filter(|runs| runs.len() <= 50)
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    runs.iter()
        .filter(|run| {
            run["headSha"] == base
                && run["headBranch"] == branch
                && run["event"] == "push"
                && run["conclusion"] == "success"
        })
        .find_map(|run| {
            Some(SelectedBaseRun {
                run_id: run["databaseId"].as_u64().filter(|id| *id > 0)?,
                attempt: run["attempt"].as_u64().filter(|attempt| *attempt > 0)?,
            })
        })
        .ok_or_else(|| "baseline_unavailable".to_owned())
}

/// Collect exact-name artifact candidates from a complete artifact listing.
///
/// The canonical name can have multiple immutable service IDs across attempts.
/// Callers must bind candidates to the requested manifest identity and
/// authenticated attempt, then require exactly one match; zero or multiple
/// matches fail closed. Each receipt binds service ID/digest and run metadata
/// independently of the deterministic manifest fingerprint.
/// # Errors
/// Rejects malformed, incomplete, or unattested listings and duplicate IDs.
pub(crate) fn select_baseline_artifacts(
    text: &str,
    expected: &str,
    base: &str,
    compatibility: &str,
    branch: &str,
    run_id: u64,
) -> Result<Vec<BaselineArtifactReceipt>, String> {
    let canonical = velnor_actions_contract::artifact_id_for_baseline(base, compatibility)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    if canonical != expected || run_id == 0 {
        return Err("baseline_unavailable".to_owned());
    }
    let entries = artifact_entries(text)?;
    let matching = entries
        .iter()
        .filter(|entry| entry["name"].as_str() == Some(expected));
    let mut receipts = Vec::new();
    for entry in matching {
        let expired = entry["expired"]
            .as_bool()
            .ok_or_else(|| "baseline_artifact_mismatch".to_owned())?;
        if expired {
            continue;
        }
        receipts.push(receipt_for_entry(
            entry,
            expected,
            compatibility,
            base,
            branch,
            run_id,
        )?);
    }
    let mut ids = std::collections::BTreeSet::new();
    if receipts
        .iter()
        .any(|receipt| !ids.insert(receipt.service_id))
    {
        return Err("baseline_artifact_ambiguous".to_owned());
    }
    Ok(receipts)
}

/// Select a named artifact when the compatibility digest is encoded in name.
///
/// Planned-baseline retrieval uses this after the plan has pinned the complete
/// canonical name. The selected run ID is independently checked against
/// service metadata; its attempt is checked after reading the manifest.
/// # Errors
/// Refuses a malformed name or any invalid service receipt.
pub(crate) fn select_named_baseline_artifacts(
    text: &str,
    expected: &str,
    base: &str,
    branch: &str,
    run_id: u64,
) -> Result<Vec<BaselineArtifactReceipt>, String> {
    let prefix = format!("velnor-baseline-{base}-");
    let compatibility = expected
        .strip_prefix(&prefix)
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    select_baseline_artifacts(text, expected, base, compatibility, branch, run_id)
}

/// Parse a complete, bounded `gh api --paginate --slurp` artifact listing.
fn artifact_entries(text: &str) -> Result<Vec<serde_json::Value>, String> {
    const MAX_RUN_ARTIFACTS: usize = 10_000;
    let value = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let pages = value
        .as_array()
        .map_or_else(|| vec![&value], |pages| pages.iter().collect());
    let mut total = None;
    let mut entries = Vec::new();
    let mut service_ids = std::collections::BTreeSet::new();
    for page in pages {
        let count = page["total_count"]
            .as_u64()
            .ok_or_else(|| "baseline_listing_incomplete".to_owned())?;
        if total.is_some_and(|previous| previous != count) {
            return Err("baseline_listing_incomplete".to_owned());
        }
        if usize::try_from(count).map_or(true, |count| count > MAX_RUN_ARTIFACTS) {
            return Err("baseline_listing_oversize".to_owned());
        }
        total = Some(count);
        let page_entries = page["artifacts"]
            .as_array()
            .ok_or_else(|| "baseline_unavailable".to_owned())?;
        for entry in page_entries {
            let service_id = entry["id"]
                .as_u64()
                .filter(|id| *id > 0)
                .ok_or_else(|| "baseline_listing_invalid".to_owned())?;
            if !service_ids.insert(service_id) {
                return Err("baseline_listing_overlap".to_owned());
            }
            entries.push(entry.clone());
            if entries.len() > MAX_RUN_ARTIFACTS {
                return Err("baseline_listing_oversize".to_owned());
            }
        }
    }
    if total != u64::try_from(entries.len()).ok() {
        return Err("baseline_listing_incomplete".to_owned());
    }
    Ok(entries)
}

/// Bind service metadata to the exact run/name and require its ZIP digest.
fn receipt_for_entry(
    entry: &serde_json::Value,
    expected: &str,
    compatibility: &str,
    base: &str,
    branch: &str,
    run_id: u64,
) -> Result<BaselineArtifactReceipt, String> {
    const MAX_BASELINE_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024;
    let size_bytes = entry["size_in_bytes"]
        .as_u64()
        .filter(|size| *size > 0 && *size <= MAX_BASELINE_ARCHIVE_BYTES)
        .ok_or_else(|| "baseline_artifact_size_invalid".to_owned())?;
    if entry["name"] != expected
        || entry["expired"].as_bool() != Some(false)
        || entry["workflow_run"]["id"].as_u64() != Some(run_id)
        || entry["workflow_run"]["head_sha"] != base
        || entry["workflow_run"]["head_branch"] != branch
    {
        return Err("baseline_artifact_mismatch".to_owned());
    }
    let service_id = entry["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| "baseline_artifact_id_missing".to_owned())?;
    let sha256 = entry["digest"]
        .as_str()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .filter(|digest| valid_sha256(digest))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "baseline_artifact_digest_missing".to_owned())?;
    Ok(BaselineArtifactReceipt {
        service_id,
        sha256,
        size_bytes,
        name: expected.to_owned(),
        compatibility_id: compatibility.to_owned(),
        run_id,
        head_sha: base.to_owned(),
        head_branch: branch.to_owned(),
    })
}

/// GitHub's service digest must be one SHA-256 hex value.
fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "run_select_tests.rs"]
mod tests;
