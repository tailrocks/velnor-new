//! Baseline evidence: naming, validation, and coverage classification.

use velnor_actions_contract::{
    BaselineStatus, Plan, WorkflowEvent, canonical_json_bytes, digest_b3, validate_digest,
};

use crate::OrchestratorError;
use crate::cover::shard;
use crate::cover_identity::{
    SOURCE_BUILD_REASON, apply_coverage, is_source_build, resolve_generator_identity,
};
use crate::decisions::baseline_expired;
use crate::internal::internal_contract;
use crate::merge::BaselineManifest;

/// Derive `velnor-baseline-<commit>-<compat>` with full IDs.
/// # Errors
pub(crate) fn baseline_artifact_name(commit: &str, compat: &str) -> Result<String, String> {
    let sha = commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit());
    reject(sha, "bad_source_commit")?;
    validate_digest(compat).map_err(|_| "bad_compatibility_id".to_owned())?;
    Ok(format!("velnor-baseline-{commit}-{compat}"))
}

/// Validate evidence: source/ref/event/run/attempt/generator/schema/digests.
pub(crate) fn validate_manifest(
    manifest: &BaselineManifest,
    base: &str,
    branch: &str,
    generator_version: &str,
    generator_sha256: &str,
) -> Result<(), String> {
    let expect = baseline_artifact_name(&manifest.source_commit, &manifest.compatibility_id)?;
    let identified = manifest.run_id > 0 && manifest.run_attempt > 0 && manifest.artifact_id > 0;
    let trusted = manifest.event == "push" && manifest.final_status == "passed";
    let generated = manifest.generator_version == generator_version
        && manifest.generator_sha256 == generator_sha256;
    let repo_ok = validate_digest(&manifest.repository_id).is_ok();
    reject(manifest.schema == 1, "stale_schema")?;
    reject(manifest.source_commit == base, "wrong_commit")?;
    reject(manifest.ref_ == format!("refs/heads/{branch}"), "wrong_ref")?;
    reject(trusted, "untrusted_proof")?;
    reject(identified, "bad_proof_identity")?;
    reject(generated, "generator_mismatch")?;
    reject(repo_ok, "bad_repository_id")?;
    reject(manifest.artifact_name == expect, "artifact_mismatch")?;
    for task in &manifest.tasks {
        let ids_ok = velnor_actions_contract::validate_task_id(&task.task_id).is_ok()
            && validate_digest(&task.task_digest).is_ok()
            && validate_digest(&task.input_digest).is_ok();
        let runs_ok = task.proof_run_id > 0 && task.observed_run_id > 0;
        let fresh_ok = task
            .external_data
            .as_ref()
            .is_none_or(|proof| proof.validate().is_ok());
        reject(ids_ok, "bad_task_identity")?;
        reject(runs_ok, "bad_proof_identity")?;
        reject(fresh_ok, "bad_external_data")?;
    }
    Ok(())
}

/// True only for protected pushes; PR/fork/merge-group runs never publish.
pub(crate) fn publish_event_eligible(event: WorkflowEvent) -> bool {
    event == WorkflowEvent::Push
}

/// Reject a failed evidence check with its reason.
fn reject(ok: bool, reason: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(reason.to_owned()) }
}

/// Current Unix time; clock failure fails closed (all dated baselines expire).
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(u64::MAX, |elapsed| elapsed.as_secs())
}

/// Classify obligations against baseline evidence, or execute everything.
///
/// A provided manifest is validated and applied; otherwise a live exact-base
/// lookup runs when the plan has a base and obligations. Coverage that would
/// invalidate the plan reverts to full execution with a warning.
/// # Errors
pub(crate) fn apply_baseline(
    plan: &mut Plan,
    event: WorkflowEvent,
    branch: &str,
    root: &std::path::Path,
    workflow: &str,
    catalog: &velnor_actions_mise::ToolCatalog,
    manifest: Option<BaselineManifest>,
) -> Result<(), OrchestratorError> {
    resolve_generator_identity(plan, root);
    let manifest = manifest.or_else(|| {
        let base = plan.base.clone()?;
        if plan.obligations.is_empty() {
            return None;
        }
        if is_source_build(&plan.generator.sha256) {
            plan.baseline.reason = Some(SOURCE_BUILD_REASON.to_owned());
            return None;
        }
        match shard::resolve_manifests(catalog, root, &base, workflow, branch) {
            Ok(found) => found.into_iter().next(),
            Err(reason) => {
                plan.baseline.reason = Some(reason);
                None
            }
        }
    });
    let Some(manifest) = manifest else {
        return Ok(());
    };
    if !publish_event_eligible(event) {
        plan.warnings.push(format!(
            "baseline_publish:forbidden:{event:?}:no_publish_attempted"
        ));
    }
    let Some(base) = plan.base.clone() else {
        plan.warnings.push("baseline_miss:missing_base".to_owned());
        return Ok(());
    };
    if let Err(reason) = validate_manifest(
        &manifest,
        &base,
        branch,
        &plan.generator.version,
        &plan.generator.sha256,
    ) {
        plan.warnings.push(format!("baseline_miss:{reason}"));
        return Ok(());
    }
    if baseline_expired(manifest.expires_at_unix, unix_now()) {
        plan.warnings.push("baseline_miss:cache_expired".to_owned());
        return Ok(());
    }
    let digest = digest_b3(&canonical_json_bytes(&manifest).map_err(internal_contract)?);
    let saved = (
        plan.obligations.clone(),
        plan.matrix.clone(),
        plan.packages.clone(),
    );
    apply_coverage(plan, &manifest, &digest, base);
    if plan.validate().is_err() {
        (plan.obligations, plan.matrix, plan.packages) = saved;
        plan.warnings
            .push("baseline_miss:plan_invalid:reverted".to_owned());
        plan.baseline.status = BaselineStatus::Unavailable;
        plan.baseline.reason = Some("baseline_unavailable".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_publish_and_naming_rules() {
        assert!(publish_event_eligible(WorkflowEvent::Push));
        assert!(!publish_event_eligible(WorkflowEvent::PullRequest));
        assert!(!publish_event_eligible(WorkflowEvent::MergeGroup));
        let name = baseline_artifact_name(&"a".repeat(40), &digest_b3(b"compat")).expect("name");
        assert!(name.starts_with("velnor-baseline-"));
        assert!(baseline_artifact_name("short", &digest_b3(b"c")).is_err());
    }
}
