//! Runtime admission for typed task prerequisites.

use std::path::Path;

use velnor_actions_contract::workflow::{
    MatrixEntry, ObligationDecision, Plan, TaskExecutionManifestEntryV1, TaskReport, TaskStatus,
};
use velnor_actions_contract::{
    EdgeKind, matrix_id_for_task_group, matrix_key_for_id, parse_strict_json,
    task_report_id_for_task,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

/// Confirm that the selected physical job owns the manifest task and lane.
pub(super) fn entry_matches_manifest(
    record: &TaskExecutionManifestEntryV1,
    entry: &MatrixEntry,
    job_id: &str,
) -> Result<bool, OrchestratorError> {
    if entry.job_id != job_id || !entry_contains_task(entry, &record.task_id) {
        return Ok(false);
    }
    if record.task_id != entry.task_id {
        return Ok(false);
    }
    let base_id =
        matrix_id_for_task_group(&entry.stack_id, &record.task_id).map_err(internal_contract)?;
    let base_key = matrix_key_for_id(&base_id).map_err(internal_contract)?;
    if record.matrix_id != base_id || record.matrix_key != base_key {
        return Ok(false);
    }
    let task_group = match entry.lane_variant {
        Some(velnor_actions_contract::NamedCheckLaneVariant::Hosted) => {
            format!("{}/lane-hosted", entry.task_id)
        }
        Some(velnor_actions_contract::NamedCheckLaneVariant::ScaleSet) => {
            format!("{}/lane-scale-set", entry.task_id)
        }
        None => entry.task_id.clone(),
    };
    let selected_id =
        matrix_id_for_task_group(&entry.stack_id, &task_group).map_err(internal_contract)?;
    let selected_key = matrix_key_for_id(&selected_id).map_err(internal_contract)?;
    if entry.id != selected_id || entry.matrix_key != selected_key {
        return Ok(false);
    }
    Ok(true)
}

/// Require every Gate and Data predecessor to have valid success evidence.
pub(super) fn validate_prerequisites(
    plan: &Plan,
    entry: &MatrixEntry,
    task_id: &str,
    job_id: &str,
    runner_temp: &Path,
) -> Result<(), OrchestratorError> {
    let incoming: Vec<&str> = plan
        .edges
        .iter()
        .filter(|edge| edge.to == task_id && matches!(edge.kind, EdgeKind::Gate | EdgeKind::Data))
        .map(|edge| edge.from.as_str())
        .collect();
    for prerequisite in incoming {
        let obligation = plan
            .obligations
            .iter()
            .find(|candidate| candidate.task_id == prerequisite)
            .ok_or_else(|| internal("task_prerequisite_without_obligation"))?;
        match obligation.decision {
            ObligationDecision::NotAffected => {
                return Err(internal("task_prerequisite_not_affected"));
            }
            ObligationDecision::Execute => {
                let source = prerequisite_entry(plan, prerequisite, entry.lane_variant)?;
                validate_success_report(plan, source, prerequisite, job_id, runner_temp)?;
            }
            ObligationDecision::ReusedFromTaskCache => {
                return Err(internal("task_prerequisite_reuse_unproved"));
            }
        }
    }
    Ok(())
}

fn entry_contains_task(entry: &MatrixEntry, task_id: &str) -> bool {
    entry
        .execute_task_ids
        .tasks
        .values()
        .any(|reference| match reference {
            velnor_actions_contract::ExecuteTaskRef::Single(id) => id == task_id,
            velnor_actions_contract::ExecuteTaskRef::Shards(ids) => {
                ids.iter().any(|id| id == task_id)
            }
        })
}

fn prerequisite_entry<'a>(
    plan: &'a Plan,
    task_id: &str,
    lane: Option<velnor_actions_contract::NamedCheckLaneVariant>,
) -> Result<&'a MatrixEntry, OrchestratorError> {
    let mut matches = plan
        .matrix
        .include
        .iter()
        .filter(|entry| entry.lane_variant == lane && entry_contains_task(entry, task_id));
    let Some(entry) = matches.next() else {
        return Err(internal("task_prerequisite_entry_missing"));
    };
    if matches.next().is_some() {
        return Err(internal("task_prerequisite_entry_ambiguous"));
    }
    Ok(entry)
}

fn validate_success_report(
    plan: &Plan,
    entry: &MatrixEntry,
    task_id: &str,
    current_job_id: &str,
    runner_temp: &Path,
) -> Result<(), OrchestratorError> {
    let obligation = plan
        .obligations
        .iter()
        .find(|candidate| candidate.task_id == task_id)
        .ok_or_else(|| internal("task_prerequisite_without_obligation"))?;
    let report_id =
        task_report_id_for_task(&plan.run_key, &entry.matrix_key, &obligation.task_digest)
            .map_err(internal_contract)?;
    let run_dir = runner_temp.join("velnor").join(&plan.run_key);
    let report_path = if entry.job_id == current_job_id {
        run_dir
            .join(&entry.matrix_key)
            .join("tasks")
            .join(format!("{report_id}.json"))
    } else {
        let home = run_dir
            .join("reports")
            .join(&entry.artifact_id)
            .join(&entry.matrix_key);
        let direct = home.join("tasks").join(format!("{report_id}.json"));
        if std::fs::symlink_metadata(&direct).is_ok() {
            direct
        } else {
            run_dir
                .join("reports")
                .join(&entry.artifact_id)
                .join(&entry.artifact_id)
                .join(&entry.matrix_key)
                .join("tasks")
                .join(format!("{report_id}.json"))
        }
    };
    let relative = report_path
        .strip_prefix(&run_dir)
        .map_err(|_| internal("task_prerequisite_path_invalid"))?;
    let relative = relative
        .to_str()
        .ok_or_else(|| internal("task_prerequisite_path_invalid"))?;
    crate::check_evidence::reject_link_components(&run_dir, relative)
        .map_err(|_| internal("task_prerequisite_report_unreadable"))?;
    let text = crate::retrieve_reports::read_staged_text(
        &report_path,
        crate::retrieve_reports::MAX_STAGED_REPORT_BYTES,
    )
    .map_err(|_| internal("task_prerequisite_report_missing_or_invalid"))?;
    let value =
        parse_strict_json(&text).map_err(|_| internal("task_prerequisite_report_unparsable"))?;
    let report: TaskReport = serde_json::from_value(value)
        .map_err(|_| internal("task_prerequisite_report_unparsable"))?;
    report
        .validate_for_plan_entry(entry)
        .map_err(|_| internal("task_prerequisite_report_invalid"))?;
    if report.run_key != plan.run_key
        || report.event != plan.event
        || report.trust != plan.trust
        || report.task_id != task_id
        || report.task_digest != obligation.task_digest
        || report.status != TaskStatus::Executed
        || report.exit_code != 0
    {
        return Err(internal("task_prerequisite_report_not_successful"));
    }
    Ok(())
}
