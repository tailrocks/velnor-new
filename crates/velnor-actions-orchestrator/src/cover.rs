//! Merge checks 2-3: report-set partition and per-entry coverage.

// Wired here (not `lib.rs`, which Gate 4-7 does not own) so the sharding
// module compiles without touching shared files.
#[path = "shard.rs"]
pub(crate) mod shard;

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    BaselineProof, BaselineStatus, ExecuteTaskRef, MatrixEntry, MatrixReport, MatrixStatus,
    ObligationDecision, Plan, PlanBaseline, TaskStatus, WorkflowEvent, canonical_json_bytes,
    digest_b3, task_report_id_for_task, validate_digest,
};

use crate::OrchestratorError;
use crate::internal::internal_contract;
use crate::merge::{BaselineManifest, MergeRequest};

/// Aggregated merge signals feeding result precedence.
#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "four precedence signals read clearest as named bools"
)]
pub(crate) struct Signals {
    /// Structural validation failed.
    pub(crate) planning_failed: bool,
    /// A required task, job, or candidate failed.
    pub(crate) failed: bool,
    /// A required task, job, or candidate was cancelled.
    pub(crate) cancelled: bool,
    /// A report is missing, malformed, duplicated, skipped, or not run.
    pub(crate) not_run: bool,
}

/// Check-2 partition of submitted reports.
pub(crate) struct Partition<'a> {
    /// First valid report per expected report ID.
    pub(crate) valid: BTreeMap<&'a str, &'a MatrixReport>,
    /// Reports failing validation or bound to another run.
    pub(crate) malformed: u32,
    /// Extra reports beyond the first per report ID.
    pub(crate) duplicates: u32,
}

/// Check 2: keep the first valid report per expected ID exactly.
pub(crate) fn partition_reports<'a>(
    request: &'a MergeRequest,
    entries: &BTreeMap<&str, &MatrixEntry>,
    signals: &mut Signals,
) -> Partition<'a> {
    let mut valid = BTreeMap::new();
    let mut malformed = 0u32;
    let mut duplicates = 0u32;
    for report in &request.matrix_reports {
        if report.run_key != request.run_key || report.validate().is_err() {
            malformed += 1;
            signals.not_run = true;
            continue;
        }
        if valid.contains_key(report.report_id.as_str()) {
            duplicates += 1;
            signals.not_run = true;
            continue;
        }
        if !entries.contains_key(report.report_id.as_str()) {
            signals.planning_failed = true;
            continue;
        }
        valid.insert(report.report_id.as_str(), report);
    }
    Partition {
        valid,
        malformed,
        duplicates,
    }
}

/// Folded task counts from covered reports.
#[derive(Debug, Default)]
pub(crate) struct Fold {
    /// Reused tasks.
    pub(crate) reused: u32,
    /// Executed tasks.
    pub(crate) executed: u32,
    /// Empty-partition tasks.
    pub(crate) empty_partition: u32,
    /// Failed tasks.
    pub(crate) failed: u32,
    /// Cancelled tasks.
    pub(crate) cancelled: u32,
    /// Not-selected (blocked) tasks.
    pub(crate) blocked: u32,
}

/// Check 3 for one entry: binding, task set, counts, digests; fold on cover.
pub(crate) fn cover_entry(
    request: &MergeRequest,
    entry: &MatrixEntry,
    report: &MatrixReport,
    obligations: &BTreeMap<&str, &str>,
    seen_task_reports: &mut BTreeSet<String>,
    fold: &mut Fold,
    signals: &mut Signals,
) -> Result<bool, OrchestratorError> {
    if report.matrix_id != entry.id || report.matrix_key != entry.matrix_key {
        signals.planning_failed = true;
        signals.not_run = true;
        return Ok(false);
    }
    if report_tasks(report) != flattened(entry) {
        signals.planning_failed = true;
        signals.not_run = true;
        return Ok(false);
    }
    let Some(recount) = recount(report) else {
        signals.not_run = true;
        return Ok(false);
    };
    if !check_digests(request, report, obligations, seen_task_reports)? {
        signals.planning_failed = true;
        signals.not_run = true;
        return Ok(false);
    }
    fold.reused += recount.reused;
    fold.executed += recount.executed;
    fold.empty_partition += recount.empty_partition;
    fold.failed += recount.failed;
    fold.cancelled += recount.cancelled;
    fold.blocked += recount.blocked;
    fold_report_status(report, signals);
    if recount.failed > 0 {
        signals.failed = true;
    }
    if recount.cancelled > 0 {
        signals.cancelled = true;
    }
    if recount.blocked > 0 {
        signals.not_run = true;
    }
    Ok(true)
}

/// Declared task IDs of one report, sorted.
fn report_tasks(report: &MatrixReport) -> Vec<&str> {
    report
        .expected_task_ids
        .iter()
        .map(String::as_str)
        .collect()
}

/// Declared task IDs of one matrix entry, sorted.
fn flattened(entry: &MatrixEntry) -> Vec<&str> {
    let mut ids = Vec::new();
    for task_ref in entry.execute_task_ids.tasks.values() {
        match task_ref {
            ExecuteTaskRef::Single(id) => ids.push(id.as_str()),
            ExecuteTaskRef::Shards(shards) => {
                ids.extend(shards.iter().map(String::as_str));
            }
        }
    }
    ids.sort_unstable();
    ids
}

/// Recounted task statuses; `None` when the summary disagrees.
fn recount(report: &MatrixReport) -> Option<Fold> {
    let mut recount = Fold::default();
    for task in &report.tasks {
        match task.status {
            TaskStatus::Reused => recount.reused += 1,
            TaskStatus::Executed => recount.executed += 1,
            TaskStatus::EmptyPartition => recount.empty_partition += 1,
            TaskStatus::NotSelected => recount.blocked += 1,
            TaskStatus::Failed => recount.failed += 1,
            TaskStatus::Cancelled => recount.cancelled += 1,
        }
    }
    let agrees = recount.reused == report.reused
        && recount.executed == report.executed
        && recount.empty_partition == report.empty_partition
        && recount.blocked == report.not_selected
        && recount.failed == report.failed
        && recount.cancelled == report.cancelled;
    agrees.then_some(recount)
}

/// Task-report IDs unique with digests matching plan obligations.
fn check_digests(
    request: &MergeRequest,
    report: &MatrixReport,
    obligations: &BTreeMap<&str, &str>,
    seen_task_reports: &mut BTreeSet<String>,
) -> Result<bool, OrchestratorError> {
    for task in &report.tasks {
        if !seen_task_reports.insert(task.task_report_id.clone()) {
            return Ok(false);
        }
        let Some(digest) = obligations.get(task.task_id.as_str()) else {
            return Ok(false);
        };
        let expected = task_report_id_for_task(&request.run_key, &report.matrix_key, digest)
            .map_err(internal_contract)?;
        if expected != task.task_report_id {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Fold one aggregate report status into signals.
fn fold_report_status(report: &MatrixReport, signals: &mut Signals) {
    match report.status {
        MatrixStatus::Failed => signals.failed = true,
        MatrixStatus::Cancelled => signals.cancelled = true,
        MatrixStatus::NotRun => signals.not_run = true,
        MatrixStatus::Passed => {}
    }
}

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
        reject(ids_ok, "bad_task_identity")?;
        reject(runs_ok, "bad_proof_identity")?;
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
    let manifest = manifest.or_else(|| {
        let base = plan.base.clone()?;
        if plan.obligations.is_empty() {
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
        plan.warnings
            .push(format!("baseline_publish:forbidden:{event:?}"));
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

/// Mark covered obligations, prune the matrix, and record the baseline.
fn apply_coverage(plan: &mut Plan, manifest: &BaselineManifest, digest: &str, base: String) {
    for obligation in &mut plan.obligations {
        let hit = manifest.tasks.iter().find(|task| {
            task.task_id == obligation.task_id
                && task.task_digest == obligation.task_digest
                && task.input_digest == obligation.input_digest
        });
        if let Some(task) = hit {
            obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
            obligation.reason = String::from("covered_by_trusted_baseline");
            obligation.baseline_proof = Some(BaselineProof {
                source_commit: manifest.source_commit.clone(),
                run_id: task.proof_run_id,
                artifact_id: manifest.artifact_id,
                artifact_name: manifest.artifact_name.clone(),
                manifest_digest: digest.to_owned(),
            });
        } else {
            plan.warnings
                .push(format!("baseline_miss:{}:no_entry", obligation.task_id));
        }
    }
    plan.matrix.include.retain(|entry| {
        plan.obligations
            .iter()
            .any(|ob| ob.task_id == entry.task_id && ob.decision == ObligationDecision::Execute)
    });
    for package in &mut plan.packages {
        package.selected = plan.obligations.iter().any(|ob| {
            ob.decision == ObligationDecision::Execute && package.tasks.contains(&ob.task_id)
        });
    }
    plan.baseline = PlanBaseline {
        status: BaselineStatus::Used,
        base_commit: Some(base),
        run_id: Some(manifest.run_id),
        artifact_id: Some(manifest.artifact_id),
        artifact_name: Some(manifest.artifact_name.clone()),
        manifest_digest: Some(digest.to_owned()),
        reason: None,
    };
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
