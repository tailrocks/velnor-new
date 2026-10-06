//! Idempotent publication: authenticate the exact-name artifact from this run.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::Plan;
use velnor_actions_mise::{RuntimePaths, ToolCatalog};

use crate::cover::shard_baseline::{self, BaselineLookup};
use crate::run_select::{BaselineArtifactReceipt, SelectedBaseRun};

/// Find the newest authenticated current-run publication without replacing it.
///
/// `None` means no reusable publication was authenticated at or before the
/// plan's current attempt. The returned wrapper retains service and API
/// receipts through qualification.
/// # Errors
/// Refuses unavailable, expired, ambiguous, malformed, or unauthenticated evidence.
pub(crate) fn retrieve_existing_publication(
    catalog: &ToolCatalog,
    root: &Path,
    plan: &Plan,
    run_id: u64,
    repo: &str,
    branch: &str,
) -> Result<Option<shard_baseline::AcquiredBaseline>, String> {
    plan.validate()
        .map_err(|_| "baseline_bad_plan".to_owned())?;
    resolve_existing(
        plan,
        run_id,
        repo,
        branch,
        |args| BaselineLookup::run(catalog, root, args),
        |lookup, receipt| {
            shard_baseline::artifact_transport::download_archive(
                catalog,
                root,
                lookup,
                receipt,
                RuntimePaths::full(),
            )
        },
    )
}

fn resolve_existing(
    plan: &Plan,
    run_id: u64,
    repo: &str,
    branch: &str,
    mut run: impl FnMut(Vec<OsString>) -> Result<String, String>,
    mut download: impl FnMut(&BaselineLookup, &BaselineArtifactReceipt) -> Result<Vec<u8>, String>,
) -> Result<Option<shard_baseline::AcquiredBaseline>, String> {
    let workflow = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    let lookup = BaselineLookup::new(&plan.head, workflow, branch, repo)?;
    let selected = SelectedBaseRun {
        run_id,
        attempt: current_run_attempt(plan, run_id)?,
    };
    let compatibility = crate::cover_compat::baseline_compat_for_plan(plan)?;
    let listed = run(lookup.artifacts_args(run_id))?;
    let name = velnor_actions_contract::artifact_id_for_baseline(&plan.head, &compatibility)
        .map_err(|_| "baseline_no_exact_artifact".to_owned())?;
    let receipts = crate::run_select::select_baseline_artifacts(
        &listed,
        &name,
        &plan.head,
        &compatibility,
        branch,
        run_id,
    )?;
    let mut successful = Vec::new();
    for receipt in receipts {
        let Ok(bytes) = download(&lookup, &receipt) else {
            continue;
        };
        let Ok(verified) = shard_baseline::artifact_transport::verify_archive(receipt, &bytes)
        else {
            continue;
        };
        let attempt_number = verified.manifest().run_attempt;
        if attempt_number == 0 || attempt_number > selected.attempt {
            continue;
        }
        let Ok(record) = run(lookup.attempt_args(run_id, attempt_number)) else {
            continue;
        };
        let Ok(acquired) =
            shard_baseline::artifact_transport::authenticate_attempt(verified, &record, &lookup)
        else {
            continue;
        };
        if acquired.attempt_receipt().run_id != run_id {
            continue;
        }
        successful.push(acquired);
    }
    let newest_attempt = successful
        .iter()
        .map(|acquired| acquired.attempt_receipt().run_attempt)
        .max();
    let Some(newest_attempt) = newest_attempt else {
        return Ok(None);
    };
    let mut latest = successful
        .into_iter()
        .filter(|acquired| acquired.attempt_receipt().run_attempt == newest_attempt);
    let Some(acquired) = latest.next() else {
        return Ok(None);
    };
    if latest.next().is_some() {
        return Err("baseline_artifact_ambiguous".to_owned());
    }
    Ok(Some(acquired))
}

/// Read the attempt bound to this exact CI run key.
fn current_run_attempt(plan: &Plan, expected_run_id: u64) -> Result<u64, String> {
    let (run_id, attempt) = plan
        .run_key
        .strip_prefix('r')
        .and_then(|value| value.split_once("-a"))
        .and_then(|(run_id, attempt)| {
            Some((run_id.parse::<u64>().ok()?, attempt.parse::<u64>().ok()?))
        })
        .ok_or_else(|| "baseline_run_key_mismatch".to_owned())?;
    if run_id != expected_run_id
        || run_id == 0
        || attempt == 0
        || plan.run_key != format!("r{run_id}-a{attempt}")
    {
        return Err("baseline_run_key_mismatch".to_owned());
    }
    Ok(attempt)
}

#[cfg(test)]
#[path = "retrieve_existing_publication_tests.rs"]
mod tests;
