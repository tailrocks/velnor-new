//! Merge checks 2-3: report-set partition and per-entry coverage.

// Wired here (not `lib.rs`, which Gate 4-7 does not own) so the sharding
// module compiles without touching shared files.
pub(crate) mod shard;
pub(crate) mod shard_baseline;
// Coverage revalidation lives apart so this file keeps its size gate.
pub(crate) mod revalidate;

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::task_report_id_for_task;
use velnor_actions_contract_workflow::{
    ExecuteTaskRef, MatrixEntry, MatrixReport, MatrixStatus, TaskReport, TaskStatus,
};

use crate::OrchestratorError;
use crate::internal::internal_contract;
use crate::internal_plan::wire_w2;
use crate::merge::MergeRequest;

pub(crate) use self::revalidate::revalidate_coverage;
pub(crate) use crate::cover_baseline::{BaselineInputs, apply_baseline};

/// Aggregated merge signals feeding result precedence.
#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "five precedence signals read clearest as named bools"
)]
pub(crate) struct Signals {
    /// Structural validation failed.
    pub(crate) planning_failed: bool,
    /// A required task, job, or candidate failed.
    pub(crate) failed: bool,
    /// A required task, job, or candidate was cancelled.
    pub(crate) cancelled: bool,
    /// A required task was blocked (`not_selected`).
    pub(crate) blocked: bool,
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
    miss_reasons: &mut BTreeSet<String>,
) -> Partition<'a> {
    let mut valid = BTreeMap::new();
    let mut malformed = 0u32;
    let mut duplicates = 0u32;
    for report in &request.matrix_reports {
        if report.run_key != request.run_key {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("trust_scope_mismatch".to_owned());
            continue;
        }
        if report.validate().is_err() {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        if valid.contains_key(report.report_id.as_str()) {
            duplicates += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        if !entries.contains_key(report.report_id.as_str()) {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
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
    /// Partitioned per-task files keyed by task-report ID.
    pub(crate) task_files: &'a BTreeMap<&'a str, &'a TaskReport>,
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
        sinks.miss_reasons.insert("cache_corrupt".to_owned());
        return Ok(false);
    }
    if report_tasks(report) != flattened(entry) {
        sinks.signals.planning_failed = true;
        sinks.signals.not_run = true;
        sinks.miss_reasons.insert("cache_corrupt".to_owned());
        return Ok(false);
    }
    let Some(recount) = recount(report) else {
        sinks.signals.not_run = true;
        sinks.miss_reasons.insert("cache_corrupt".to_owned());
        return Ok(false);
    };
    if !check_digests(
        request,
        report,
        obligations,
        &entry.declared_outputs,
        &mut *sinks.seen_task_reports,
        &mut *sinks.miss_reasons,
    )? {
        sinks.signals.planning_failed = true;
        sinks.signals.not_run = true;
        return Ok(false);
    }
    if !check_task_files(report, obligations, &entry.declared_outputs, &mut *sinks) {
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
        sinks.signals.blocked = true;
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
    declared: &[String],
    seen_task_reports: &mut BTreeSet<String>,
    miss_reasons: &mut BTreeSet<String>,
) -> Result<bool, OrchestratorError> {
    for task in &report.tasks {
        if !seen_task_reports.insert(task.task_report_id.clone()) {
            miss_reasons.insert("cache_corrupt".to_owned());
            return Ok(false);
        }
        let Some(digest) = obligations.get(task.task_id.as_str()) else {
            miss_reasons.insert("cache_corrupt".to_owned());
            return Ok(false);
        };
        let expected = task_report_id_for_task(&request.run_key, &report.matrix_key, digest)
            .map_err(internal_contract)?;
        if expected != task.task_report_id {
            miss_reasons.insert("cache_corrupt".to_owned());
            return Ok(false);
        }
        // Task reports carry no restore observations yet (P04); reuse
        // claims fail closed until the contract carries them.
        let observed: &[(String, Vec<u8>, String)] = &[];
        if task.status == TaskStatus::Reused
            && let Err(reason) =
                wire_w2::verify_reused_task(&task.task_id, declared, observed, None, None, None)
        {
            miss_reasons.insert(reason.as_str().to_owned());
            return Ok(false);
        }
    }
    Ok(true)
}

/// Every aggregate task entry backed by its validated task file.
///
/// The aggregate alone never proves its tasks: each entry needs the
/// partitioned file with matching task identity, plan digest, status,
/// exit code, and declared-only outputs. A missing file is absent
/// evidence; any contradiction corrupts the set. Exit coherence is
/// structural: `Executed` must carry exit 0 and `Failed` a nonzero
/// exit, and entry and file must agree on both fields.
fn check_task_files(
    report: &MatrixReport,
    obligations: &BTreeMap<&str, &str>,
    declared: &[String],
    sinks: &mut CoverSinks<'_>,
) -> bool {
    for task in &report.tasks {
        let Some(file) = sinks.task_files.get(task.task_report_id.as_str()) else {
            sinks.signals.planning_failed = true;
            sinks.miss_reasons.insert("source_missing".to_owned());
            return false;
        };
        let digest_ok = obligations
            .get(task.task_id.as_str())
            .is_some_and(|digest| file.task_digest == **digest);
        let coherent = file.task_id == task.task_id
            && file.status == task.status
            && file.exit_code == task.exit_code
            && exit_coherent(file.status, file.exit_code)
            && file.matrix_id == report.matrix_id
            && file.matrix_key == report.matrix_key
            && file.validate_outputs_declared(declared).is_ok();
        if !digest_ok || !coherent {
            sinks.signals.planning_failed = true;
            sinks.miss_reasons.insert("cache_corrupt".to_owned());
            return false;
        }
    }
    true
}

/// Exit code coherent with the task status.
///
/// `Executed` proves exit 0; `Failed` proves a nonzero exit. Other
/// statuses carry producer-defined exits and stay unconstrained here.
fn exit_coherent(status: TaskStatus, exit_code: i32) -> bool {
    match status {
        TaskStatus::Executed => exit_code == 0,
        TaskStatus::Failed => exit_code != 0,
        TaskStatus::Reused
        | TaskStatus::EmptyPartition
        | TaskStatus::NotSelected
        | TaskStatus::Cancelled => true,
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
