//! Publisher parent acquisition pinned to the plan's original evidence.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::{Plan, canonical_json_bytes, digest_b3};
use velnor_actions_mise::ToolCatalog;

use crate::cover::shard_baseline::{self, BaselineLookup};
use crate::merge::BaselineManifest;
use crate::run_select::BaselineArtifactReceipt;

/// Fetch only the plan-selected parent, bypassing arbitrary staged input.
///
/// The exact service attempt authenticates the downloaded manifest, so a
/// later run or rerun for the same source cannot replace selected evidence.
pub(crate) fn retrieve_planned_baseline_to(
    catalog: &ToolCatalog,
    run_dir: &Path,
    plan: &serde_json::Value,
    repo: &str,
) -> bool {
    let Ok(plan) = serde_json::from_value::<Plan>(plan.clone()) else {
        return false;
    };
    retrieve_typed_baseline_to(catalog, run_dir, &plan, repo)
}

/// Shared typed acquisition for publisher and Required; no latest-run relookup.
pub(super) fn retrieve_typed_baseline_to(
    catalog: &ToolCatalog,
    run_dir: &Path,
    plan: &Plan,
    repo: &str,
) -> bool {
    if plan.validate().is_err() || ParentClaim::from_plan(plan).is_none() {
        return false;
    }
    let Some(branch) = super::default_branch_for(catalog, run_dir, repo) else {
        return false;
    };
    let Some(parent) = resolve_planned(
        plan,
        repo,
        &branch,
        |args| BaselineLookup::run(catalog, run_dir, args),
        |lookup, receipt| {
            shard_baseline::artifact_transport::download_archive(
                catalog,
                run_dir,
                lookup,
                receipt,
                velnor_actions_mise::RuntimePaths::full(),
            )
        },
    ) else {
        return false;
    };
    super::stage_manifest(run_dir, parent.manifest())
}

/// Exact claim extracted only after strict typed plan validation.
struct ParentClaim {
    base: String,
    run_id: u64,
    artifact_id: u64,
    name: String,
    digest: String,
}

impl ParentClaim {
    fn from_plan(plan: &Plan) -> Option<Self> {
        if !crate::covered_tasks::plan_has_covered(plan) {
            return None;
        }
        let raw = serde_json::to_value(&plan.baseline).ok()?;
        let claim = Self {
            base: raw.get("base_commit")?.as_str()?.to_owned(),
            run_id: raw.get("run_id")?.as_u64()?,
            artifact_id: raw.get("artifact_id")?.as_u64()?,
            name: raw.get("artifact_name")?.as_str()?.to_owned(),
            digest: raw.get("manifest_digest")?.as_str()?.to_owned(),
        };
        let compatibility = crate::cover_compat::baseline_compat_for_plan(plan).ok()?;
        let derived =
            velnor_actions_contract::artifact_id_for_baseline(&claim.base, &compatibility).ok()?;
        if plan.base.as_deref() != Some(&claim.base)
            || claim.name != derived
            || claim.artifact_id != crate::cover_compat::baseline_artifact_numeric_id(&derived)
        {
            return None;
        }
        Some(claim)
    }

    fn matches(&self, manifest: &BaselineManifest) -> bool {
        manifest.source_commit == self.base
            && manifest.run_id == self.run_id
            && manifest.artifact_id == self.artifact_id
            && manifest.artifact_name == self.name
            && canonical_json_bytes(manifest).is_ok_and(|bytes| digest_b3(&bytes) == self.digest)
    }
}

/// Service-bound acquisition; injected transport enables real race regressions.
pub(super) fn resolve_planned(
    plan: &Plan,
    repo: &str,
    branch: &str,
    mut run: impl FnMut(Vec<OsString>) -> Result<String, String>,
    mut download: impl FnMut(&BaselineLookup, &BaselineArtifactReceipt) -> Result<Vec<u8>, String>,
) -> Option<shard_baseline::AcquiredBaseline> {
    let claim = ParentClaim::from_plan(plan)?;
    let workflow = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    let lookup = BaselineLookup::new(&claim.base, workflow, branch, repo).ok()?;
    let listed = run(lookup.artifacts_args(claim.run_id)).ok()?;
    let receipts = crate::run_select::select_named_baseline_artifacts(
        &listed,
        &claim.name,
        &claim.base,
        branch,
        claim.run_id,
    )
    .ok()?;
    let mut acquired_match = None;
    for receipt in receipts {
        let Ok(bytes) = download(&lookup, &receipt) else {
            continue;
        };
        let Ok(verified) = shard_baseline::artifact_transport::verify_archive(receipt, &bytes)
        else {
            continue;
        };
        if !claim.matches(verified.manifest()) {
            continue;
        }
        let attempt = verified.manifest().run_attempt;
        let Ok(record) = run(lookup.attempt_args(claim.run_id, attempt)) else {
            continue;
        };
        let Ok(acquired) =
            shard_baseline::artifact_transport::authenticate_attempt(verified, &record, &lookup)
        else {
            continue;
        };
        if acquired_match.replace(acquired).is_some() {
            return None;
        }
    }
    acquired_match
}

/// GitHub's exact attempt record independently binds publication provenance.
pub(crate) fn authentic_attempt(
    text: &str,
    manifest: &BaselineManifest,
    repo: &str,
    branch: &str,
    workflow: &str,
) -> bool {
    let Ok(run) = velnor_actions_contract::parse_strict_json(text) else {
        return false;
    };
    let Some((workflow_repo, workflow_path, workflow_ref)) =
        crate::cover_baseline::provenance_check::parse_workflow_ref(&manifest.workflow_ref)
    else {
        return false;
    };
    run.get("id").and_then(serde_json::Value::as_u64) == Some(manifest.run_id)
        && run.get("run_attempt").and_then(serde_json::Value::as_u64) == Some(manifest.run_attempt)
        && manifest.run_attempt > 0
        && run["head_sha"] == manifest.source_commit
        && run["event"] == "push"
        && run["status"] == "completed"
        && run["conclusion"] == "success"
        && run["head_branch"] == branch
        && crate::cover::shard_baseline::artifact_transport::attempt_path_matches(
            &run["path"],
            workflow,
            branch,
        )
        && run["repository"]["full_name"]
            .as_str()
            .and_then(crate::origin::validate_repository_slug)
            == crate::origin::validate_repository_slug(repo)
        && crate::origin::validate_repository_slug(&workflow_repo)
            == crate::origin::validate_repository_slug(repo)
        && workflow_path == workflow
        && workflow_ref == format!("refs/heads/{branch}")
        && manifest.ref_ == format!("refs/heads/{branch}")
        && manifest.repository_id == digest_b3(format!("github.com/{repo}").as_bytes())
}

#[cfg(test)]
#[path = "retrieve_planned_baseline_tests.rs"]
mod tests;
