//! Event-time `fetch-reports-v1`: exact crate-artifact retrieval.
//!
//! The final job's retrieve step runs before request assembly: it reads
//! the downloaded plan, then downloads each expected crate artifact by exact
//! derived name with pinned `gh` (`gh run download <run-id> --name
//! <artifact-id> --dir reports/<artifact-id>`), never a wildcard. Each
//! leg retries transient failures up to
//! [`MAX_DOWNLOAD_ATTEMPTS`](velnor_actions_orchestrator_retrieve_retry::retrieve_retry::MAX_DOWNLOAD_ATTEMPTS);
//! persistent failure skips that job's entries (merge judges
//! `not_run`). A missing or unparsable plan downloads nothing and
//! still exits success so the merge reaches its `planning_failed`
//! verdict. Only unusable environment (no runner temp, no numeric
//! run ID) fails outright.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use velnor_actions_contract::{parse_strict_json, validate_artifact_id};
use velnor_actions_mise::ToolCatalog;

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal_contract;
use velnor_actions_orchestrator_core::staged_reads::read_staged_text;
use velnor_actions_orchestrator_retrieve_retry::retrieve_retry::download_with_retry;

/// Download each plan-expected artifact into `reports/<artifact-id>/`.
///
/// A missing or unparsable plan means zero downloads (the merge still
/// runs and reports `planning_failed`). Malformed artifact IDs are
/// skipped without spawning; failed downloads are skipped per leg.
/// Afterwards the plan's exact baseline is best-effort fetched beside
/// them; the merge judges its absence as `source_missing` itself.
pub fn retrieve_reports_to(run_id: u64, run_dir: &Path) -> usize {
    let plan = read_plan(run_dir);
    let Some(plan) = plan else {
        return 0;
    };
    let repo = std::env::var(velnor_actions_orchestrator_core::origin::GITHUB_REPOSITORY_ENV)
        .ok()
        .and_then(|raw| velnor_actions_orchestrator_core::origin::validate_repository_slug(&raw));
    let Some(repo) = repo else {
        return 0;
    };
    let catalog = ToolCatalog::pinned();
    let mut retrieved = 0usize;
    for artifact_id in expected_artifact_ids(&plan) {
        // Validate BEFORE join/mkdir: a malformed ID must never become
        // a path or a directory (X7).
        if validate_artifact_id(artifact_id).is_err() {
            continue;
        }
        let dir = run_dir.join("reports").join(artifact_id);
        if fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let Ok(args) = retrieve_args(run_id, artifact_id, &dir, &repo) else {
            continue;
        };
        let (downloaded, _) = download_with_retry(|| {
            velnor_actions_orchestrator_cover::cover::shard::BaselineLookup::run(
                &catalog,
                run_dir,
                args.clone(),
            )
            .is_ok()
        });
        if downloaded {
            retrieved += 1;
        }
    }
    crate::retrieve_baseline::retrieve_baseline_to(&catalog, run_dir, &plan, &repo);
    retrieved
}

/// Fixed `gh run download` argv for one exact artifact (no wildcards).
///
/// `--repo` pins the download to the expected repository: without it
/// `gh` would resolve the repo from the working directory's git
/// origin, which a prior step may have rewritten.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for a malformed artifact ID
/// or a malformed repository slug.
pub fn retrieve_args(
    run_id: u64,
    artifact_id: &str,
    dir: &Path,
    repo: &str,
) -> Result<Vec<OsString>, OrchestratorError> {
    validate_artifact_id(artifact_id).map_err(internal_contract)?;
    let Some(repo) = velnor_actions_orchestrator_core::origin::validate_repository_slug(repo)
    else {
        return Err(internal_contract(
            velnor_actions_contract::ContractError::identity("repository", "bad_lookup_repo"),
        ));
    };
    Ok(vec![
        OsString::from("run"),
        OsString::from("download"),
        OsString::from(run_id.to_string()),
        OsString::from("--name"),
        OsString::from(artifact_id),
        OsString::from("--dir"),
        dir.as_os_str().to_owned(),
        OsString::from("--repo"),
        OsString::from(repo),
    ])
}

/// Maximum bytes read for the retrieve-step plan.
///
/// Matches the merge-request assembly bound: the same `plan.json`
/// parses identically at retrieve and merge time, and a giant plan
/// downloads nothing instead of exhausting the final job's memory.
pub const MAX_RETRIEVE_PLAN_BYTES: u64 = 4 << 20;

/// Parse the downloaded plan, if any.
///
/// Symlink-rejecting, size-bounded, duplicate-key-rejecting (X7); typed
/// plan structs additionally carry `deny_unknown_fields`. Bounded like
/// every other event-time read: a missing, oversize, or unparsable plan
/// downloads nothing, and the merge still reaches its `planning_failed`
/// verdict.
fn read_plan(run_dir: &Path) -> Option<serde_json::Value> {
    let text = read_staged_text(&run_dir.join("plan.json"), MAX_RETRIEVE_PLAN_BYTES).ok()?;
    parse_strict_json(&text).ok()
}

/// Expected job artifact IDs from a plan value, in plan order.
///
/// Sibling entries share their job's artifact, so the enumeration
/// dedupes: each artifact downloads exactly once while first-seen
/// plan order stays stable.
fn expected_artifact_ids(plan: &serde_json::Value) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(entries) = plan
        .get("matrix")
        .and_then(|matrix| matrix.get("include"))
        .and_then(serde_json::Value::as_array)
    {
        for entry in entries {
            if let Some(id) = entry.get("artifact_id").and_then(serde_json::Value::as_str)
                && seen.insert(id)
            {
                ids.push(id);
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests;
