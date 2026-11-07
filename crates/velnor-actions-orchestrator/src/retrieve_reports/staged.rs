// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "dual_lane.rs"]
mod dual_lane;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use velnor_actions_contract::staged_reads::{path_is_symlink, read_staged_text};
use velnor_actions_contract::{
    parse_strict_json, task_report_id_for_task, validate_artifact_id, validate_matrix_key,
};

use dual_lane::read_dual_lane_crate_entry;

const MAX_STAGED_REPORT_BYTES: u64 = 1 << 20;

pub(crate) fn read_staged_reports(
    run_key: &str,
    plan: &serde_json::Value,
    dir: &Path,
    errors: &mut Vec<String>,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let digests = obligation_digests(plan);
    let entries = plan
        .get("matrix")
        .and_then(|matrix| matrix.get("include"))
        .and_then(serde_json::Value::as_array);
    let mut reports = Vec::new();
    let mut tasks = Vec::new();
    for entry in entries.into_iter().flatten() {
        let Some(artifact_id) = entry.get("artifact_id").and_then(serde_json::Value::as_str) else {
            errors.push("bad_artifact_id".to_owned());
            continue;
        };
        if validate_artifact_id(artifact_id).is_err() {
            errors.push(format!("bad_artifact_id:{artifact_id}"));
            continue;
        }
        let Some(matrix_key) = entry.get("matrix_key").and_then(serde_json::Value::as_str) else {
            errors.push(format!("bad_matrix_key:{artifact_id}"));
            continue;
        };
        if validate_matrix_key(matrix_key).is_err() {
            errors.push(format!("bad_matrix_key:{artifact_id}"));
            continue;
        }
        let hosted_artifact = format!(
            "{artifact_id}{}",
            velnor_actions_contract::workflow::lanes::HOSTED_SUFFIX
        );
        let local_artifact = format!(
            "{artifact_id}{}",
            velnor_actions_contract::workflow::lanes::SCALE_SUFFIX
        );
        if artifact_id.starts_with("velnor-crate-")
            && !dir.join(artifact_id).exists()
            && (dir.join(&hosted_artifact).exists() || dir.join(&local_artifact).exists())
        {
            read_dual_lane_crate_entry(
                run_key,
                plan,
                entry,
                dir,
                &hosted_artifact,
                &local_artifact,
                matrix_key,
                &digests,
                &mut reports,
                &mut tasks,
                errors,
            );
            continue;
        }
        if let Some((report, entry_tasks)) = read_single_lane_entry(
            run_key,
            plan,
            entry,
            dir,
            artifact_id,
            matrix_key,
            &digests,
            errors,
        ) {
            reports.push(report);
            tasks.extend(entry_tasks);
        }
    }
    reports.sort_by(|left, right| staged_id(left, "report_id").cmp(staged_id(right, "report_id")));
    tasks.sort_by(|left, right| {
        staged_id(left, "task_report_id").cmp(staged_id(right, "task_report_id"))
    });
    (reports, tasks)
}

#[expect(
    clippy::too_many_arguments,
    reason = "single-lane report reader validates matrix report, observation bindings, and expected task reports"
)]
pub(super) fn read_single_lane_entry(
    run_key: &str,
    plan: &serde_json::Value,
    entry: &serde_json::Value,
    dir: &Path,
    artifact_id: &str,
    matrix_key: &str,
    digests: &BTreeMap<&str, &str>,
    errors: &mut Vec<String>,
) -> Option<(serde_json::Value, Vec<serde_json::Value>)> {
    let mut lane_reports = Vec::new();
    let home = read_matrix_file(dir, artifact_id, matrix_key, &mut lane_reports, errors)?;
    let report = lane_reports.pop()?;
    check_staged_observations(run_key, plan, entry, matrix_key, &home, digests, errors);
    let tasks_dir = home.join("tasks");
    let linked = path_is_symlink(&tasks_dir);
    let mut lane_tasks = Vec::new();
    for report_id in expected_file_ids(entry, run_key, digests) {
        if linked {
            errors.push(format!("symlink_task:{report_id}"));
            continue;
        }
        match read_bounded(&tasks_dir.join(format!("{report_id}.json"))) {
            Ok(text) => match parse_strict_json(&text) {
                Ok(task) => lane_tasks.push(task),
                Err(_) => errors.push(format!("unparsable_task:{report_id}")),
            },
            Err("missing") => errors.push(format!("missing_task:{report_id}")),
            Err(kind) => errors.push(format!("{kind}_task:{report_id}")),
        }
    }
    Some((report, lane_tasks))
}

fn check_staged_observations(
    run_key: &str,
    plan: &serde_json::Value,
    entry: &serde_json::Value,
    matrix_key: &str,
    home: &Path,
    digests: &BTreeMap<&str, &str>,
    errors: &mut Vec<String>,
) {
    let source = plan
        .get("head")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let artifact = entry
        .get("artifact_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let job = entry
        .get("job_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let attempt = crate::task_report::run_attempt(run_key);
    let tasks_dir = home.join("tasks");
    let linked = path_is_symlink(&tasks_dir);
    for report_id in expected_file_ids(entry, run_key, digests) {
        if linked {
            errors.push(format!("symlink_observation:{report_id}"));
            continue;
        }
        let Ok(member) = crate::task_report::unit_observation_member_name(matrix_key, &report_id)
        else {
            errors.push(format!("corrupt_observation:{report_id}"));
            continue;
        };
        let expectation = crate::task_report::UnitObservationExpectation {
            task_report_id: report_id.clone(),
            artifact_id: artifact.to_owned(),
            member_name: member,
            producer_job: job.to_owned(),
            producer_attempt: attempt,
            source_sha: source.to_owned(),
        };
        check_staged_observation(&expectation, &tasks_dir, errors);
    }
}

fn check_staged_observation(
    expectation: &crate::task_report::UnitObservationExpectation,
    tasks_dir: &Path,
    errors: &mut Vec<String>,
) {
    let report_id = expectation.task_report_id.as_str();
    let path = tasks_dir.join(format!(
        "{report_id}{}",
        velnor_actions_contract::workflow::UNIT_OBSERVATION_SUFFIX
    ));
    let text = match read_bounded(&path) {
        Ok(text) => text,
        Err("missing") => return,
        Err("symlink") => {
            errors.push(format!("symlink_observation:{report_id}"));
            return;
        }
        Err("oversize") => {
            errors.push(format!("oversize_observation:{report_id}"));
            return;
        }
        Err(_) => {
            errors.push(format!("unreadable_observation:{report_id}"));
            return;
        }
    };
    let Ok(binding) = crate::task_report::parse_unit_observation_binding(&text) else {
        errors.push(format!("unparsable_observation:{report_id}"));
        return;
    };
    if crate::task_report::verify_unit_observation_binding(&binding, expectation).is_err() {
        errors.push(format!("corrupt_observation:{report_id}"));
    }
}

fn obligation_digests(plan: &serde_json::Value) -> BTreeMap<&str, &str> {
    let mut digests = BTreeMap::new();
    let obligations = plan
        .get("obligations")
        .and_then(serde_json::Value::as_array);
    for obligation in obligations.into_iter().flatten() {
        let id = obligation
            .get("task_id")
            .and_then(serde_json::Value::as_str);
        let digest = obligation
            .get("task_digest")
            .and_then(serde_json::Value::as_str);
        if let (Some(id), Some(digest)) = (id, digest) {
            digests.insert(id, digest);
        }
    }
    digests
}

fn read_matrix_file(
    dir: &Path,
    artifact_id: &str,
    matrix_key: &str,
    reports: &mut Vec<serde_json::Value>,
    errors: &mut Vec<String>,
) -> Option<PathBuf> {
    let home = dir.join(artifact_id);
    if path_is_symlink(&home) {
        errors.push(format!("symlink_report:{artifact_id}"));
        return None;
    }
    let entry_dir = home.join(matrix_key);
    if path_is_symlink(&entry_dir) {
        errors.push(format!("symlink_report:{artifact_id}"));
        return None;
    }
    let direct = entry_dir.join("matrix-report.json");
    let nested_entry = home.join(artifact_id).join(matrix_key);
    let nested = nested_entry.join("matrix-report.json");
    let (path, text) = match read_bounded(&direct) {
        Ok(text) => (direct, text),
        Err("missing") => {
            if path_is_symlink(&home.join(artifact_id)) || path_is_symlink(&nested_entry) {
                errors.push(format!("symlink_report:{artifact_id}"));
                return None;
            }
            match read_bounded(&nested) {
                Ok(text) => (nested, text),
                Err(kind) => {
                    errors.push(format!("{kind}_report:{artifact_id}"));
                    return None;
                }
            }
        }
        Err(kind) => {
            errors.push(format!("{kind}_report:{artifact_id}"));
            return None;
        }
    };
    let Ok(report) = parse_strict_json(&text) else {
        errors.push(format!("unparsable_report:{artifact_id}"));
        return None;
    };
    reports.push(report);
    path.parent().map(Path::to_path_buf)
}

fn expected_file_ids(
    entry: &serde_json::Value,
    run_key: &str,
    digests: &BTreeMap<&str, &str>,
) -> Vec<String> {
    let Some(matrix_key) = entry.get("matrix_key").and_then(serde_json::Value::as_str) else {
        return Vec::new();
    };
    let refs = entry
        .get("execute_task_ids")
        .and_then(|ids| ids.get("tasks").or(Some(ids)))
        .and_then(serde_json::Value::as_object);
    let mut ids = Vec::new();
    for task_ref in refs.into_iter().flat_map(|tasks| tasks.values()) {
        let mut push = |id: &str| {
            let derived = digests
                .get(id)
                .and_then(|digest| task_report_id_for_task(run_key, matrix_key, digest).ok());
            ids.extend(derived);
        };
        if let Some(id) = task_ref.as_str() {
            push(id);
        } else if let Some(shards) = task_ref.as_array() {
            for shard in shards.iter().filter_map(serde_json::Value::as_str) {
                push(shard);
            }
        }
    }
    ids.sort();
    ids
}

fn read_bounded(path: &Path) -> Result<String, &'static str> {
    read_staged_text(path, MAX_STAGED_REPORT_BYTES)
}

pub(super) fn staged_id<'a>(report: &'a serde_json::Value, field: &str) -> &'a str {
    report
        .get(field)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}
