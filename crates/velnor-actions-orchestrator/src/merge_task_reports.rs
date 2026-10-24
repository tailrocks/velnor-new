//! Plan-bound task report inventory and partitioning.

use super::{BTreeMap, BTreeSet, ExecuteTaskRef, MergeRequest, Plan, Signals};
use velnor_actions_contract::{TaskReport, task_report_id_for_task};

/// Obligation task digests keyed by task ID.
pub(crate) fn plan_digests(plan: &Plan) -> BTreeMap<&str, &str> {
    plan.obligations
        .iter()
        .map(|obligation| (obligation.task_id.as_str(), obligation.task_digest.as_str()))
        .collect()
}

/// Plan-derived per-task file expectation: report ID to task identity.
///
/// The expectation derives from the plan alone (leg task IDs plus
/// obligation digests), never from an aggregate summary, so a lying
/// `matrix-report.json` cannot shrink or redirect the file set. Legs
/// naming tasks without an obligation digest stay underivable here;
/// per-entry coverage fails them closed.
pub(crate) fn expected_task_reports(
    plan: &Plan,
    run_key: &str,
) -> BTreeMap<String, (String, String)> {
    let digests = plan_digests(plan);
    let mut expected = BTreeMap::new();
    for entry in &plan.matrix.include {
        for task_ref in entry.execute_task_ids.tasks.values() {
            let ids: &[String] = match task_ref {
                ExecuteTaskRef::Single(id) => std::slice::from_ref(id),
                ExecuteTaskRef::Shards(ids) => ids.as_slice(),
            };
            for id in ids {
                let derived = digests.get(id.as_str()).and_then(|digest| {
                    task_report_id_for_task(run_key, &entry.matrix_key, digest).ok()
                });
                if let Some(report_id) = derived {
                    expected.insert(report_id, (id.clone(), entry.matrix_key.clone()));
                }
            }
        }
    }
    expected
}

/// Check-2 partition of embedded per-task reports.
pub(crate) struct TaskPartition<'a> {
    /// First valid report per expected task-report ID.
    pub(crate) valid: BTreeMap<&'a str, &'a TaskReport>,
    /// Reports failing validation or bound to another run.
    pub(crate) malformed: u32,
    /// Extra reports beyond the first per task-report ID.
    pub(crate) duplicates: u32,
}

/// Check 2 for task files: first valid report per expected ID exactly.
///
/// Mirrors the matrix partition: malformed and duplicate files are
/// `not_run` (never success); valid files outside the plan-derived
/// expectation corrupt the set. Missing files surface per entry.
pub(crate) fn partition_task_reports<'a>(
    plan: &Plan,
    request: &'a MergeRequest,
    expected: &BTreeMap<String, (String, String)>,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) -> TaskPartition<'a> {
    let mut valid = BTreeMap::new();
    let mut malformed = 0u32;
    let mut duplicates = 0u32;
    for report in &request.task_reports {
        if report.run_key != request.run_key {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("trust_scope_mismatch".to_owned());
            continue;
        }
        // Malformed files are `not_run` even when unexpected: shape
        // triage precedes the plan-membership check.
        if report.validate().is_err() {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        let Some((expected_task_id, expected_matrix_key)) = expected.get(&report.task_report_id)
        else {
            signals.planning_failed = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        };
        if report.task_id != *expected_task_id || report.matrix_key != *expected_matrix_key {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        let Some(entry) = plan
            .matrix
            .include
            .iter()
            .find(|entry| entry.matrix_key == *expected_matrix_key)
        else {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        };
        if report.validate_for_plan_entry(entry).is_err() {
            malformed += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        if valid.contains_key(report.task_report_id.as_str()) {
            duplicates += 1;
            signals.not_run = true;
            miss_reasons.insert("cache_corrupt".to_owned());
            continue;
        }
        valid.insert(report.task_report_id.as_str(), report);
    }
    TaskPartition {
        valid,
        malformed,
        duplicates,
    }
}
