//! Publisher parent acquisition pinned to the plan's original evidence.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::{Plan, canonical_json_bytes, digest_b3};
use velnor_actions_mise::ToolCatalog;

use crate::cover::shard::BaselineLookup;
use crate::merge::BaselineManifest;

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
        |args| BaselineLookup::run_archive(catalog, run_dir, args),
    ) else {
        return false;
    };
    super::stage_manifest(run_dir, &parent)
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
        let derived = crate::cover_baseline::lookup_artifact_name(plan, &claim.base).ok()?;
        (plan.base.as_deref() == Some(&claim.base)
            && claim.name == derived
            && claim.artifact_id == crate::cover_compat::baseline_artifact_numeric_id(&derived))
        .then_some(claim)
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
    mut run_archive: impl FnMut(Vec<OsString>) -> Result<Vec<u8>, String>,
) -> Option<BaselineManifest> {
    let claim = ParentClaim::from_plan(plan)?;
    let workflow = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    let lookup = BaselineLookup::new(&claim.base, workflow, branch, repo).ok()?;
    let metadata = crate::baseline_artifact_listing::select_metadata(
        &lookup.repo,
        claim.run_id,
        &claim.name,
        &mut run,
    )
    .ok()?;
    let temp = tempfile::tempdir().ok()?;
    let archive = run_archive(lookup.artifact_zip_args(metadata.id).ok()?).ok()?;
    crate::cover::shard_baseline::archive::stage_baseline_archive(
        temp.path(),
        &claim.name,
        &metadata,
        &archive,
    )
    .ok()?;
    let payload = read_parent(temp.path(), &claim)?;
    let endpoint = format!(
        "repos/{repo}/actions/runs/{}/attempts/{}",
        claim.run_id, payload.run_attempt
    );
    let text = run(vec![OsString::from("api"), OsString::from(endpoint)]).ok()?;
    authentic_attempt(&text, &payload, repo, branch, workflow).then_some(payload)
}

/// Exact single-file archive layout and digest, never a staged plan parent.
fn read_parent(dir: &Path, claim: &ParentClaim) -> Option<BaselineManifest> {
    let entry = dir.join(&claim.name);
    let text = crate::retrieve_reports::read_staged_text(
        &entry.join("baseline.json"),
        crate::retrieve_reports::MAX_STAGED_REPORT_BYTES,
    )
    .ok()?;
    let value = crate::internal_plan::snapshot::parse_canonical_json(&text).ok()?;
    let candidate: BaselineManifest = serde_json::from_value(value).ok()?;
    let manifest = crate::cover_baseline::baseline_entry_for(
        &entry,
        &claim.base,
        claim.run_id,
        candidate.run_attempt,
        claim.artifact_id,
    )?;
    claim.matches(&manifest).then_some(manifest)
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
        && run["path"]
            .as_str()
            .is_some_and(|path| attempt_path_matches(path, workflow, branch))
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

/// Accept the API's bare path or its documented `path@branch` form only.
fn attempt_path_matches(actual: &str, workflow: &str, branch: &str) -> bool {
    if actual == workflow {
        return true;
    }
    let Some(suffix) = actual
        .strip_prefix(workflow)
        .and_then(|remainder| remainder.strip_prefix('@'))
    else {
        return false;
    };
    suffix == branch || suffix == format!("refs/heads/{branch}")
}

#[cfg(test)]
#[path = "retrieve_planned_baseline_tests.rs"]
mod tests;
