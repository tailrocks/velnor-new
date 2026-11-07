// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_contract::{
    ExecuteTaskRef, MatrixEntry, MatrixReport, TaskReport, TaskStatus, task_report_id_for_task,
};

use super::{read_single_lane_entry, staged_id};

#[expect(
    clippy::too_many_arguments,
    reason = "dual-lane crate staging threads run context, both lane artifact IDs, and output accumulators"
)]
pub(super) fn read_dual_lane_crate_entry(
    run_key: &str,
    plan: &serde_json::Value,
    entry: &serde_json::Value,
    dir: &Path,
    hosted_artifact: &str,
    local_artifact: &str,
    matrix_key: &str,
    digests: &BTreeMap<&str, &str>,
    reports: &mut Vec<serde_json::Value>,
    tasks: &mut Vec<serde_json::Value>,
    errors: &mut Vec<String>,
) {
    let hosted_err_start = errors.len();
    let hosted = read_single_lane_entry(
        run_key,
        plan,
        entry,
        dir,
        hosted_artifact,
        matrix_key,
        digests,
        errors,
    );
    let hosted_clean = errors.len() == hosted_err_start;
    let local_err_start = errors.len();
    let local = read_single_lane_entry(
        run_key,
        plan,
        entry,
        dir,
        local_artifact,
        matrix_key,
        digests,
        errors,
    );
    let local_clean = errors.len() == local_err_start;
    let hosted_valid = hosted.as_ref().is_some_and(|(report, lane_tasks)| {
        validate_lane_entry(run_key, entry, report, lane_tasks, digests)
    });
    if hosted.is_some() && !hosted_valid {
        errors.push(format!("corrupt_report:{hosted_artifact}"));
    }
    let local_valid = local.as_ref().is_some_and(|(report, lane_tasks)| {
        validate_lane_entry(run_key, entry, report, lane_tasks, digests)
    });
    if local.is_some() && !local_valid {
        errors.push(format!("corrupt_report:{local_artifact}"));
    }
    let (Some((hosted_report, hosted_tasks)), Some((local_report, local_tasks))) = (hosted, local)
    else {
        return;
    };
    if !hosted_clean || !local_clean || !hosted_valid || !local_valid {
        return;
    }
    if staged_id(&hosted_report, "status") != staged_id(&local_report, "status") {
        let base_artifact = hosted_artifact
            .strip_suffix(velnor_actions_contract::workflow::lanes::HOSTED_SUFFIX)
            .unwrap_or(hosted_artifact);
        errors.push(format!("corrupt_dual_lane_mismatch:{base_artifact}"));
    }
    let local_failed = staged_id(&local_report, "status") != "passed"
        || local_tasks
            .iter()
            .any(|task| staged_id(task, "status") != "executed");
    if local_failed {
        reports.push(local_report);
        tasks.extend(local_tasks);
    } else {
        reports.push(hosted_report);
        tasks.extend(hosted_tasks);
    }
}

fn validate_lane_entry(
    run_key: &str,
    entry_val: &serde_json::Value,
    report_val: &serde_json::Value,
    tasks_val: &[serde_json::Value],
    digests: &BTreeMap<&str, &str>,
) -> bool {
    let Ok(entry) = serde_json::from_value::<MatrixEntry>(entry_val.clone()) else {
        return false;
    };
    let Ok(report) = serde_json::from_value::<MatrixReport>(report_val.clone()) else {
        return false;
    };
    if report.run_key != run_key
        || report.report_id != entry.report_id
        || report.matrix_id != entry.id
        || report.matrix_key != entry.matrix_key
        || report.validate().is_err()
        || report_tasks(&report) != flattened(&entry)
        || !recount_matches(&report)
        || tasks_val.len() != report.tasks.len()
    {
        return false;
    }
    let Some(task_files) = validate_task_files(run_key, &entry, &report, tasks_val) else {
        return false;
    };
    validate_report_tasks(run_key, &report, digests, &task_files)
}

fn validate_task_files(
    run_key: &str,
    entry: &MatrixEntry,
    report: &MatrixReport,
    tasks_val: &[serde_json::Value],
) -> Option<BTreeMap<String, TaskReport>> {
    let mut task_files = BTreeMap::new();
    for task_val in tasks_val {
        let file = serde_json::from_value::<TaskReport>(task_val.clone()).ok()?;
        if file.run_key != run_key
            || file.validate().is_err()
            || file.validate_for_plan_entry(entry).is_err()
            || file
                .validate_outputs_declared(&entry.declared_outputs)
                .is_err()
            || file.matrix_id != report.matrix_id
            || file.matrix_key != report.matrix_key
            || !exit_coherent(file.status, file.exit_code)
            || task_files
                .insert(file.task_report_id.clone(), file)
                .is_some()
        {
            return None;
        }
    }
    Some(task_files)
}

fn validate_report_tasks(
    run_key: &str,
    report: &MatrixReport,
    digests: &BTreeMap<&str, &str>,
    task_files: &BTreeMap<String, TaskReport>,
) -> bool {
    let mut seen_reports = BTreeSet::new();
    for task in &report.tasks {
        if !seen_reports.insert(task.task_report_id.as_str()) {
            return false;
        }
        let Some(digest) = digests.get(task.task_id.as_str()) else {
            return false;
        };
        let Ok(expected_id) = task_report_id_for_task(run_key, &report.matrix_key, digest) else {
            return false;
        };
        let Some(file) = task_files.get(&task.task_report_id) else {
            return false;
        };
        if expected_id != task.task_report_id
            || file.task_digest != *digest
            || file.task_id != task.task_id
            || file.status != task.status
            || file.exit_code != task.exit_code
        {
            return false;
        }
    }
    true
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
            ExecuteTaskRef::Shards(shards) => ids.extend(shards.iter().map(String::as_str)),
        }
    }
    ids.sort_unstable();
    ids
}

fn recount_matches(report: &MatrixReport) -> bool {
    let (mut reused, mut executed, mut empty, mut not_sel, mut failed, mut cancelled) =
        (0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    for task in &report.tasks {
        match task.status {
            TaskStatus::Reused => reused += 1,
            TaskStatus::Executed => executed += 1,
            TaskStatus::EmptyPartition => empty += 1,
            TaskStatus::NotSelected => not_sel += 1,
            TaskStatus::Failed => failed += 1,
            TaskStatus::Cancelled => cancelled += 1,
        }
    }
    reused == report.reused
        && executed == report.executed
        && empty == report.empty_partition
        && not_sel == report.not_selected
        && failed == report.failed
        && cancelled == report.cancelled
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
