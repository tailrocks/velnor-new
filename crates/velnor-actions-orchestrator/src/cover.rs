//! Merge checks 2-3: report-set partition and per-entry coverage.

// Wired here (not `lib.rs`, which Gate 4-7 does not own) so the sharding
// module compiles without touching shared files.
#[path = "shard.rs"]
pub(crate) mod shard;

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ExecuteTaskRef, MatrixEntry, MatrixReport, MatrixStatus, ObligationDecision, Plan,
    PlanObligation, TaskStatus, canonical_json_bytes, digest_b3, task_report_id_for_task,
};

use crate::OrchestratorError;
use crate::internal::internal_contract;
use crate::internal_plan::wire_w2;
use crate::merge::{BaselineManifest, MergeRequest};

pub(crate) use crate::cover_baseline::{BaselineInputs, apply_baseline};

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

/// Mutable merge sinks threaded through per-entry coverage checks.
#[derive(Debug)]
pub(crate) struct CoverSinks<'a> {
    /// Seen task-report IDs.
    pub(crate) seen_task_reports: &'a mut BTreeSet<String>,
    /// Folded task counts.
    pub(crate) fold: &'a mut Fold,
    /// Merge signals.
    pub(crate) signals: &'a mut Signals,
    /// Miss reasons for uncovered tasks.
    pub(crate) miss_reasons: &'a mut BTreeSet<String>,
}

/// Check 3 for one entry: binding, task set, counts, digests; fold on cover.
pub(crate) fn cover_entry(
    request: &MergeRequest,
    entry: &MatrixEntry,
    report: &MatrixReport,
    obligations: &BTreeMap<&str, &str>,
    sinks: &mut CoverSinks<'_>,
) -> Result<bool, OrchestratorError> {
    if report.matrix_id != entry.id || report.matrix_key != entry.matrix_key {
        sinks.signals.planning_failed = true;
        sinks.signals.not_run = true;
        return Ok(false);
    }
    if report_tasks(report) != flattened(entry) {
        sinks.signals.planning_failed = true;
        sinks.signals.not_run = true;
        return Ok(false);
    }
    let Some(recount) = recount(report) else {
        sinks.signals.not_run = true;
        return Ok(false);
    };
    if !check_digests(
        request,
        report,
        obligations,
        &mut *sinks.seen_task_reports,
        &mut *sinks.miss_reasons,
    )? {
        sinks.signals.planning_failed = true;
        sinks.signals.not_run = true;
        return Ok(false);
    }
    sinks.fold.reused += recount.reused;
    sinks.fold.executed += recount.executed;
    sinks.fold.empty_partition += recount.empty_partition;
    sinks.fold.failed += recount.failed;
    sinks.fold.cancelled += recount.cancelled;
    sinks.fold.blocked += recount.blocked;
    fold_report_status(report, &mut *sinks.signals);
    if recount.failed > 0 {
        sinks.signals.failed = true;
    }
    if recount.cancelled > 0 {
        sinks.signals.cancelled = true;
    }
    if recount.blocked > 0 {
        sinks.signals.not_run = true;
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
    miss_reasons: &mut BTreeSet<String>,
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
        if task.status == TaskStatus::Reused
            && let Err(reason) = wire_w2::verify_reused_task(&task.task_id)
        {
            miss_reasons.insert(reason.as_str().to_owned());
            return Ok(false);
        }
    }
    Ok(true)
}

/// Revalidate planner coverage claims against the trusted manifest.
pub(crate) fn revalidate_coverage(
    plan: &Plan,
    manifest: Option<&BaselineManifest>,
    signals: &mut Signals,
) {
    let covered: Vec<&PlanObligation> = plan
        .obligations
        .iter()
        .filter(|ob| ob.decision == ObligationDecision::CoveredByTrustedBaseline)
        .collect();
    if covered.is_empty() {
        return;
    }
    let Some(manifest) = manifest else {
        signals.planning_failed = true;
        return;
    };
    for obligation in covered {
        let Some(proof) = &obligation.baseline_proof else {
            signals.planning_failed = true;
            continue;
        };
        let hit = manifest
            .tasks
            .iter()
            .find(|task| task.task_id == obligation.task_id);
        let Some(task) = hit else {
            signals.planning_failed = true;
            continue;
        };
        let bound = task.task_digest == obligation.task_digest
            && task.input_digest == obligation.input_digest
            && proof.run_id == task.proof_run_id
            && proof.artifact_name == manifest.artifact_name;
        // A serialization failure is planning_failed, never a digest over an
        // empty default that could verify against a forged proof.
        match canonical_json_bytes(manifest) {
            Ok(bytes) if bound && proof.manifest_digest == digest_b3(&bytes) => {}
            _ => signals.planning_failed = true,
        }
    }
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
