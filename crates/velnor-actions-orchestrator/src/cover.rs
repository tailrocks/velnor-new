#[path = "shard.rs"]
pub(crate) mod shard;

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ExecuteTaskRef, MatrixEntry, MatrixReport, MatrixStatus, TaskReport, TaskStatus,
    task_report_id_for_task,
};

use crate::OrchestratorError;
use crate::internal::internal_contract;
use crate::internal_plan::wire_w2;
use crate::merge::MergeRequest;

#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "five precedence signals read clearest as named bools"
)]
pub(crate) struct Signals {
    pub(crate) planning_failed: bool,
    pub(crate) failed: bool,
    pub(crate) cancelled: bool,
    pub(crate) blocked: bool,
    pub(crate) not_run: bool,
}

pub(crate) struct Partition<'a> {
    pub(crate) valid: BTreeMap<&'a str, &'a MatrixReport>,
    pub(crate) malformed: u32,
    pub(crate) duplicates: u32,
}

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

#[derive(Debug, Default)]
pub(crate) struct Fold {
    pub(crate) reused: u32,
    pub(crate) executed: u32,
    pub(crate) empty_partition: u32,
    pub(crate) failed: u32,
    pub(crate) cancelled: u32,
    pub(crate) blocked: u32,
}

#[derive(Debug)]
pub(crate) struct CoverSinks<'a> {
    pub(crate) seen_task_reports: &'a mut BTreeSet<String>,
    pub(crate) task_files: &'a BTreeMap<&'a str, &'a TaskReport>,
    pub(crate) fold: &'a mut Fold,
    pub(crate) signals: &'a mut Signals,
    pub(crate) miss_reasons: &'a mut BTreeSet<String>,
}

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

fn report_tasks(report: &MatrixReport) -> Vec<&str> {
    report
        .expected_task_ids
        .iter()
        .map(String::as_str)
        .collect()
}

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

fn fold_report_status(report: &MatrixReport, signals: &mut Signals) {
    match report.status {
        MatrixStatus::Failed => signals.failed = true,
        MatrixStatus::Cancelled => signals.cancelled = true,
        MatrixStatus::NotRun => signals.not_run = true,
        MatrixStatus::Passed => {}
    }
}
