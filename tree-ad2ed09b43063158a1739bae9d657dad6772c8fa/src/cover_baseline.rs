//! Baseline evidence: naming, validation, and coverage classification.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    BaselineStatus, Plan, WorkflowEvent, canonical_json_bytes, digest_b3, validate_digest,
};
use velnor_actions_mise::BaselineLookup as MiseBaselineLookup;

use crate::OrchestratorError;
use crate::cover::shard;
use crate::cover_identity::{
    SOURCE_BUILD_REASON, apply_coverage, is_source_build, resolve_generator_identity,
};
use crate::decisions::baseline_expired;
use crate::discover::Discovery;
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
        if let Some(proof) = &task.proof {
            proof.validate().map_err(|_| "bad_task_proof".to_owned())?;
            let bound = proof.task_id == task.task_id
                && proof.task_digest == task.task_digest
                && proof.input_digest == task.input_digest
                && proof.proof_run_id == task.proof_run_id;
            reject(bound, "proof_mismatch")?;
            continue;
        }
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
/// lookup runs when the plan has a base and obligations. Coverage that would
/// invalidate the plan reverts to full execution with a warning.
/// # Errors
pub(crate) fn apply_baseline(
    plan: &mut Plan,
    event: WorkflowEvent,
    inputs: BaselineInputs<'_>,
    manifest: Option<BaselineManifest>,
    discovery: &Discovery,
) -> Result<(), OrchestratorError> {
    resolve_generator_identity(plan, inputs.root);
    let manifest = manifest.or_else(|| {
        let base = plan.base.clone()?;
        if plan.obligations.is_empty() {
            return None;
        }
        if is_source_build(&plan.generator.sha256) {
            plan.baseline.reason = Some(SOURCE_BUILD_REASON.to_owned());
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
        inputs.branch,
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
    apply_coverage(plan, &manifest, &digest, base, discovery);
    if plan.validate().is_err() {
        (plan.obligations, plan.matrix, plan.packages) = saved;
        plan.warnings
            .push("baseline_miss:plan_invalid:reverted".to_owned());
        plan.baseline.status = BaselineStatus::Unavailable;
        plan.baseline.reason = Some("baseline_unavailable".to_owned());
    }
    Ok(())
}

/// Baseline download argv: exact-name when the artifact is known (PAR-5.10).
///
/// The expected name needs the compatibility digest, which is unknown
/// until a manifest validates; without it the whole run downloads and
/// exact filtering happens in [`baseline_entry_for`].
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
    shard::BaselineLookup::download_args(run_id, dir)
}

/// One exact-base baseline from a download entry (PAR-5.5).
///
/// The entry directory must carry exactly `baseline.json` (single-file
/// UTF-8, no other payload) with matching source commit and artifact name.
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
    let text = std::fs::read_to_string(payload?).ok()?;
    let manifest: BaselineManifest = serde_json::from_str(&text).ok()?;
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
    fn baseline_publish_and_naming_rules() {
        assert!(publish_event_eligible(WorkflowEvent::Push));
        assert!(!publish_event_eligible(WorkflowEvent::PullRequest));
        assert!(!publish_event_eligible(WorkflowEvent::MergeGroup));
        let name = baseline_artifact_name(&"a".repeat(40), &digest_b3(b"compat")).expect("name");
        assert!(name.starts_with("velnor-baseline-"));
        assert!(baseline_artifact_name("short", &digest_b3(b"c")).is_err());
    }

    #[test]
    fn download_args_name_exact_artifact_when_known() {
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
        let whole: Vec<String> = baseline_download_args(&base, "w", "b", None, 7, dir)
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(!whole.contains(&"--name".to_owned()));
    }

    #[test]
    fn baseline_entry_needs_single_utf8_payload() {
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
