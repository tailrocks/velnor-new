//! Baseline evidence: resolution, validation, and coverage classification.

// Wired here so provenance checks compile without touching `lib.rs`.
pub(crate) mod provenance_check;
pub(crate) mod provenance_resolve;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use std::collections::BTreeSet;

use velnor_actions_contract::{canonical_json_bytes, digest_b3, validate_digest};
use velnor_actions_contract_workflow::{Plan, PlanBaseline, WorkflowEvent};
use velnor_actions_mise::BaselineLookup as MiseBaselineLookup;

use self::provenance_check::{
    ProvenanceExpectations, publish_event_eligible, repository_slug_from_origin,
    validate_provenance,
};
use self::provenance_resolve::{repository_anchor_for_slug, resolve_expected_repository};
use crate::OrchestratorError;
use crate::cover::shard;
use crate::cover_identity::{SOURCE_BUILD_REASON, apply_coverage, is_source_build};
use crate::decisions::baseline_expired;
use crate::discover::Discovery;
use crate::internal::internal_contract;
use crate::merge::BaselineManifest;

/// Maximum accepted `baseline.json` bytes: evidence stays bounded.
const MAX_BASELINE_MANIFEST_BYTES: usize = 1_048_576;

/// Current Unix time; clock failure fails closed (all dated baselines expire).
pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(u64::MAX, |elapsed| elapsed.as_secs())
}

/// Baseline resolution inputs: branch, root, workflow, catalog, and repo.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BaselineInputs<'a> {
    /// Default branch for manifest refs.
    pub(crate) branch: &'a str,
    /// Repository root for manifest lookup.
    pub(crate) root: &'a Path,
    /// Workflow path for manifest lookup.
    pub(crate) workflow: &'a str,
    /// Tool catalog for manifest resolution.
    pub(crate) catalog: &'a velnor_actions_mise::ToolCatalog,
    /// Runner-owned repository slug (`owner/repo`) from the request.
    ///
    /// `None` models a local run: the git origin is the fallback.
    /// Callers pass this explicitly so baseline classification stays a
    /// pure function of request plus checkout; ambient process env
    /// must never leak in here (it once poisoned CI runs whose
    /// runner env described a different repository than the target).
    pub(crate) repository: Option<&'a str>,
}

/// Classify obligations against baseline evidence, or execute everything.
///
/// A provided manifest is validated and applied; otherwise a live exact-base
/// lookup runs when the plan has a base and obligations. Every miss keeps
/// full execution and records its exact reason: cache misses, corruption,
/// and expiry broaden instead of failing, while coverage that would
/// invalidate the plan reverts to full execution with a warning.
/// Validation runs fresh on every call; reports never replay cached
/// verdicts.
/// # Errors
pub(crate) fn apply_baseline(
    plan: &mut Plan,
    event: WorkflowEvent,
    inputs: BaselineInputs<'_>,
    manifest: Option<BaselineManifest>,
    discovery: &Discovery,
    changed: Option<&BTreeSet<String>>,
) -> Result<(), OrchestratorError> {
    // No lock fill: the plan generator always names the running binary,
    // so a source build can never emit a release-pinned identity.
    let manifest = manifest.or_else(|| lookup_manifest(plan, inputs));
    let Some(manifest) = manifest else {
        // `lookup_manifest` already recorded the precise miss reason
        // (`baseline_no_base`, source-build, lookup error); only fall back
        // to `baseline_not_found` when it marked nothing.
        if plan.baseline.reason().is_none() {
            mark_unavailable(plan, "baseline_not_found");
        }
        return Ok(());
    };
    if !publish_event_eligible(event) {
        plan.warnings.push(format!(
            "baseline_publish:forbidden:{event:?}:no_publish_attempted"
        ));
    }
    let Some(base) = plan.base.clone() else {
        mark_unavailable(plan, "baseline_no_base");
        plan.warnings.push("baseline_miss:missing_base".to_owned());
        return Ok(());
    };
    let digest = digest_b3(&canonical_json_bytes(&manifest).map_err(internal_contract)?);
    let origin = repository_slug_from_origin(inputs.root);
    let repo = resolve_expected_repository(origin.as_deref(), inputs.repository);
    let expected = ProvenanceExpectations {
        base: base.clone(),
        branch: inputs.branch.to_owned(),
        workflow_path: inputs.workflow.to_owned(),
        generator_version: plan.generator.version.clone(),
        generator_sha256: plan.generator.sha256.clone(),
        repository_id: repo.slug.as_deref().map(repository_anchor_for_slug),
        repository_slug: repo.slug,
        repository_conflict: repo.conflict,
    };
    let provenance = match validate_provenance(&manifest, &digest, &expected) {
        Ok(provenance) => provenance,
        Err(reason) => {
            mark_unavailable(plan, &format!("baseline_invalid:{reason}"));
            plan.warnings.push(format!("baseline_miss:{reason}"));
            return Ok(());
        }
    };
    if baseline_expired(manifest.expires_at_unix, unix_now()) {
        mark_unavailable(plan, "baseline_expired");
        plan.warnings.push("baseline_miss:cache_expired".to_owned());
        return Ok(());
    }
    let saved = (
        plan.obligations.clone(),
        plan.matrix.clone(),
        plan.packages.clone(),
    );
    let covered = apply_coverage(plan, &manifest, &provenance, discovery, changed, &inputs);
    if plan.validate().is_err() {
        (plan.obligations, plan.matrix, plan.packages) = saved;
        plan.warnings
            .push("baseline_miss:plan_invalid:reverted".to_owned());
        mark_unavailable(plan, "baseline_unavailable");
    } else if covered == 0 {
        mark_unavailable(plan, "baseline_no_entries_matched");
    } else {
        plan.baseline = PlanBaseline::used(
            &provenance.source_commit,
            provenance.run_id,
            provenance.artifact_id,
            &provenance.artifact_name,
            &provenance.manifest_digest,
        )
        .map_err(internal_contract)?;
    }
    Ok(())
}

/// Exact baseline artifact name for one plan over its base.
///
/// Compatibility derives from the plan's obligation set alone, so the
/// lookup names the same artifact the protected push published without
/// caller input. A malformed base or an underivable shape misses
/// before spawning anything.
pub(crate) fn lookup_artifact_name(plan: &Plan, base: &str) -> Result<String, String> {
    let compat = crate::cover_compat::baseline_compat_for_plan(plan)?;
    velnor_actions_contract::artifact_id_for_baseline(base, &compat)
        .map_err(|_| "baseline_no_exact_artifact".to_owned())
}

/// Mark baseline evidence unavailable with an explicit reason.
fn mark_unavailable(plan: &mut Plan, reason: &str) {
    if plan.baseline.mark_unavailable(reason).is_err() {
        plan.warnings.push("baseline_mark_failed".to_owned());
    }
}

/// Live exact-base lookup when the caller supplied no manifest.
///
/// Every miss records its reason on the plan; `None` means execute-all.
fn lookup_manifest(plan: &mut Plan, inputs: BaselineInputs<'_>) -> Option<BaselineManifest> {
    let Some(base) = plan.base.clone() else {
        mark_unavailable(plan, "baseline_no_base");
        return None;
    };
    if plan.obligations.is_empty() {
        mark_unavailable(plan, "baseline_no_obligations");
        return None;
    }
    if is_source_build(&plan.generator.sha256) {
        mark_unavailable(plan, SOURCE_BUILD_REASON);
        return None;
    }
    let artifact = match lookup_artifact_name(plan, &base) {
        Ok(name) => name,
        Err(reason) => {
            mark_unavailable(plan, &reason);
            return None;
        }
    };
    match shard::resolve_manifests(
        inputs.catalog,
        inputs.root,
        &base,
        inputs.workflow,
        inputs.branch,
        Some(&artifact),
        inputs.repository,
    ) {
        Ok(found) => {
            let mut found = found.into_iter();
            let first = found.next();
            if first.is_none() {
                mark_unavailable(plan, "baseline_not_found");
            }
            first
        }
        Err(reason) => {
            mark_unavailable(plan, &reason);
            None
        }
    }
}

/// Baseline download argv for one exact artifact (PAR-5.10).
///
/// Only exact-name downloads exist: without a known artifact name there
/// is no bounded download, so no command is returned and the lookup
/// fails closed to execute-all. The whole-run download fallback is gone.
/// `--repo` pins the download to the expected repository, and a
/// malformed repo slug yields no command instead of an unscoped one.
pub(crate) fn baseline_download_args(
    base: &str,
    workflow: &str,
    branch: &str,
    artifact: Option<&str>,
    run_id: u64,
    dir: &Path,
    repo: &str,
) -> Vec<OsString> {
    if let Some(name) = artifact.filter(|name| !name.is_empty())
        && let Ok(lookup) = MiseBaselineLookup::new(base, workflow, branch, name)
        && let Some(repo) = crate::origin::validate_repository_slug(repo)
    {
        let mut args = lookup.download_args(run_id, dir);
        args.push(OsString::from("--repo"));
        args.push(OsString::from(repo));
        return args;
    }
    Vec::new()
}

/// One exact-base baseline from a download entry (PAR-5.5).
///
/// The entry directory must carry exactly `baseline.json` (single-file
/// bounded UTF-8, no other payload) with matching source commit, run,
/// attempt, artifact id, and artifact name. The entry dir and the
/// payload reject symlinks (the payload opens `NOFOLLOW` and validates
/// via the open handle through the shared staged reader); reads stop
/// past the size bound, duplicate JSON keys are rejected (never
/// last-wins), and old canonical schemas fail closed via the migration
/// gate.
pub(crate) fn baseline_entry_for(
    dir: &Path,
    base: &str,
    expected_run_id: u64,
    expected_attempt: u64,
    expected_artifact_id: u64,
) -> Option<BaselineManifest> {
    let name = dir.file_name()?.to_str()?;
    let rest = name.strip_prefix(&format!("velnor-baseline-{base}-"))?;
    if validate_digest(rest).is_err() {
        return None;
    }
    if crate::retrieve_reports::path_is_symlink(dir) {
        return None;
    }
    let mut count = 0u32;
    let mut payload: Option<PathBuf> = None;
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        count += 1;
        if entry.file_name() == "baseline.json" {
            payload = Some(entry.path());
        }
    }
    if count != 1 {
        return None;
    }
    let payload = payload?;
    if payload.file_name()?.to_str()? != "baseline.json" {
        return None;
    }
    let bound = u64::try_from(MAX_BASELINE_MANIFEST_BYTES).unwrap_or(u64::MAX);
    let bytes = crate::retrieve_reports::read_staged_bytes(&payload, bound).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let value = crate::internal_plan::snapshot::parse_canonical_json(text).ok()?;
    let manifest: BaselineManifest = serde_json::from_value(value).ok()?;
    if crate::internal_plan::snapshot::check_canonical_version(manifest.schema).is_err() {
        return None;
    }
    if manifest.source_commit == base
        && manifest.run_id == expected_run_id
        && manifest.run_attempt == expected_attempt
        && manifest.artifact_id == expected_artifact_id
        && manifest.artifact_name == name
    {
        Some(manifest)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
