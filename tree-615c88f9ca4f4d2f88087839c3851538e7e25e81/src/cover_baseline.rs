//! Baseline evidence: resolution, validation, and coverage classification.

// Wired here so provenance checks compile without touching `lib.rs`.
#[path = "provenance_check.rs"]
pub(crate) mod provenance_check;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use std::collections::BTreeSet;

use velnor_actions_contract::{
    BaselineStatus, Plan, PlanBaseline, WorkflowEvent, canonical_json_bytes, digest_b3,
    validate_digest,
};
use velnor_actions_mise::BaselineLookup as MiseBaselineLookup;

use self::provenance_check::{
    ProvenanceExpectations, publish_event_eligible, repository_anchor_from_origin,
    validate_provenance,
};
use crate::OrchestratorError;
use crate::cover::shard;
use crate::cover_identity::{
    SOURCE_BUILD_REASON, apply_coverage, is_source_build, resolve_generator_identity,
};
use crate::decisions::baseline_expired;
use crate::discover::Discovery;
use crate::internal::internal_contract;
use crate::merge::BaselineManifest;

/// Maximum accepted `baseline.json` bytes: evidence stays bounded.
const MAX_BASELINE_MANIFEST_BYTES: usize = 1_048_576;

/// Current Unix time; clock failure fails closed (all dated baselines expire).
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(u64::MAX, |elapsed| elapsed.as_secs())
}

/// Baseline resolution inputs: branch, root, workflow, and tool catalog.
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
    resolve_generator_identity(plan, inputs.root);
    let manifest = manifest.or_else(|| lookup_manifest(plan, inputs));
    let Some(manifest) = manifest else {
        plan.baseline.status = BaselineStatus::Unavailable;
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
    let expected = ProvenanceExpectations {
        base: base.clone(),
        branch: inputs.branch.to_owned(),
        workflow_path: inputs.workflow.to_owned(),
        generator_version: plan.generator.version.clone(),
        generator_sha256: plan.generator.sha256.clone(),
        repository_id: repository_anchor_from_origin(inputs.root),
    };
    let provenance = match validate_provenance(&manifest, &digest, &expected) {
        Ok(provenance) => provenance,
        Err(reason) => {
            mark_unavailable(plan, &format!("baseline_invalid:{reason}"));
            plan.warnings.push(format!("baseline_miss:{reason}"));
            return Ok(());
        }
    };
    if expected.repository_id.is_none() {
        plan.warnings
            .push("baseline_repository_unverified:no_git_origin".to_owned());
    }
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
        plan.baseline = PlanBaseline {
            status: BaselineStatus::Used,
            base_commit: Some(provenance.source_commit),
            run_id: Some(provenance.run_id),
            artifact_id: Some(provenance.artifact_id),
            artifact_name: Some(provenance.artifact_name),
            manifest_digest: Some(provenance.manifest_digest),
            reason: None,
        };
    }
    Ok(())
}

/// Mark baseline evidence unavailable with an explicit reason.
fn mark_unavailable(plan: &mut Plan, reason: &str) {
    plan.baseline.status = BaselineStatus::Unavailable;
    plan.baseline.reason = Some(reason.to_owned());
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
    match shard::resolve_manifests(
        inputs.catalog,
        inputs.root,
        &base,
        inputs.workflow,
        inputs.branch,
        None,
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
pub(crate) fn baseline_download_args(
    base: &str,
    workflow: &str,
    branch: &str,
    artifact: Option<&str>,
    run_id: u64,
    dir: &Path,
) -> Vec<OsString> {
    if let Some(name) = artifact.filter(|name| !name.is_empty())
        && let Ok(lookup) = MiseBaselineLookup::new(base, workflow, branch, name)
    {
        return lookup.download_args(run_id, dir);
    }
    Vec::new()
}

/// One exact-base baseline from a download entry (PAR-5.5).
///
/// The entry directory must carry exactly `baseline.json` (single-file
/// bounded UTF-8, no other payload) with matching source commit and
/// artifact name. Duplicate JSON keys are rejected, never last-wins.
pub(crate) fn baseline_entry_for(dir: &Path, base: &str) -> Option<BaselineManifest> {
    let name = dir.file_name()?.to_str()?;
    let rest = name.strip_prefix(&format!("velnor-baseline-{base}-"))?;
    if validate_digest(rest).is_err() {
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
    let bytes = std::fs::read(payload?).ok()?;
    if bytes.len() > MAX_BASELINE_MANIFEST_BYTES {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    let value = crate::internal_plan::snapshot::parse_canonical_json(text).ok()?;
    let manifest: BaselineManifest = serde_json::from_value(value).ok()?;
    if manifest.source_commit == base && manifest.artifact_name == name {
        Some(manifest)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_publish_and_download_rules() {
        assert!(publish_event_eligible(WorkflowEvent::Push));
        assert!(!publish_event_eligible(WorkflowEvent::PullRequest));
        assert!(!publish_event_eligible(WorkflowEvent::MergeGroup));
        let base = "a".repeat(40);
        let dir = Path::new("/tmp/x");
        let name = format!("velnor-baseline-{base}-{}", digest_b3(b"c"));
        let named: Vec<String> = baseline_download_args(
            &base,
            ".github/workflows/velnor.yml",
            "testmain",
            Some(&name),
            7,
            dir,
        )
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
        assert_eq!(&named[0..4], &["run", "download", "7", "--name"]);
        assert_eq!(named[4], name);
        assert!(baseline_download_args(&base, "w", "b", None, 7, dir).is_empty());
        assert!(baseline_download_args(&base, "w", "b", Some(""), 7, dir).is_empty());
    }

    #[test]
    fn baseline_entry_needs_single_strict_payload() {
        let base = "a".repeat(40);
        let name = format!("velnor-baseline-{base}-{}", digest_b3(b"c"));
        let tmp = tempfile::tempdir().expect("tempdir");
        let entry = tmp.path().join(&name);
        std::fs::create_dir(&entry).expect("entry");
        std::fs::write(entry.join("baseline.json"), "{}").expect("json");
        std::fs::write(entry.join("extra.json"), "{}").expect("extra");
        assert!(baseline_entry_for(&entry, &base).is_none());
        std::fs::remove_file(entry.join("extra.json")).expect("rm");
        assert!(baseline_entry_for(&entry, &base).is_none());
        std::fs::write(entry.join("baseline.json"), r#"{"schema": 1, "schema": 1}"#).expect("dup");
        assert!(baseline_entry_for(&entry, &base).is_none());
        std::fs::write(entry.join("baseline.json"), [0xff, 0xfe]).expect("bad");
        assert!(baseline_entry_for(&entry, &base).is_none());
        let digest = digest_b3(b"d");
        let manifest = serde_json::json!({
            "schema": 1,
            "repository_id": digest,
            "source_commit": base,
            "ref": "refs/heads/testmain",
            "event": "push",
            "workflow_ref": "o/r/.github/workflows/velnor.yml@refs/heads/testmain",
            "run_id": 7,
            "run_attempt": 1,
            "final_status": "passed",
            "generator_version": "0.1.0",
            "generator_sha256": "1".repeat(64),
            "compatibility_id": digest,
            "artifact_id": 9,
            "artifact_name": name,
            "tasks": [],
        });
        std::fs::write(entry.join("baseline.json"), manifest.to_string()).expect("manifest");
        let found = baseline_entry_for(&entry, &base).expect("entry");
        assert_eq!(found.artifact_name, name);
    }
}
