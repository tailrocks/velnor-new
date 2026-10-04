//! Exact planned Action API evidence inventory and bounded staged reads.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    ActionBinding, ActionOutcome, MatrixEntry, ObligationDecision, Plan, parse_strict_json,
    validate_artifact_id, validate_matrix_key,
};

use super::MergeRequest;
use crate::cover::Signals;
use crate::retrieve_reports::{path_is_symlink, read_staged_text};

/// Docker build remains an Action API obligation even if its descriptor is absent.
fn needs_action(entry: &MatrixEntry) -> bool {
    entry.task_id.starts_with("stack/workload/") && entry.task_id.ends_with("/build/docker_build")
        || entry.adapter_metadata.get("action").is_some()
        || (entry.stack_id == "workload"
            && entry
                .adapter_metadata
                .get("configuration")
                .and_then(serde_json::Value::as_str)
                == Some("docker_build")
            && entry
                .adapter_metadata
                .get("kind")
                .and_then(serde_json::Value::as_str)
                == Some("build"))
}

/// Baseline coverage has no action execution; normal merge validates its proof.
pub(crate) fn executes_action(plan: &Plan, entry: &MatrixEntry) -> bool {
    needs_action(entry)
        && plan
            .obligations
            .iter()
            .find(|ob| ob.task_id == entry.task_id)
            .is_none_or(|ob| ob.decision != ObligationDecision::CoveredByTrustedBaseline)
}

/// Derive the binding from the validated plan and closed producer contract.
fn expected_binding(plan: &Plan, entry: &MatrixEntry) -> Option<ActionBinding> {
    let obligation = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == entry.task_id)?;
    if obligation.decision != ObligationDecision::Execute {
        return None;
    }
    let action = format!("velnor-action-{}", entry.matrix_key);
    crate::action_report::binding(plan, entry, &obligation.task_digest, &action).ok()
}

/// Prove exact action inventory and source binding before accepting coverage.
pub(crate) fn check_actions(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss: &mut BTreeSet<String>,
) {
    let mut expected = BTreeSet::new();
    for entry in plan
        .matrix
        .include
        .iter()
        .filter(|entry| executes_action(plan, entry))
    {
        expected.insert(entry.matrix_key.as_str());
        let Some(binding) = expected_binding(plan, entry) else {
            reject(signals, miss, "cache_corrupt");
            continue;
        };
        let begins: Vec<_> = request
            .action_begins
            .iter()
            .filter(|begin| begin.matrix_key == entry.matrix_key)
            .collect();
        let reports: Vec<_> = request
            .action_reports
            .iter()
            .filter(|report| report.binding.matrix_key == entry.matrix_key)
            .collect();
        if begins.len() != 1 || reports.len() != 1 {
            reject(signals, miss, "source_missing");
            continue;
        }
        if begins[0].validate().is_err() || reports[0].validate().is_err() {
            reject(signals, miss, "cache_corrupt");
            continue;
        }
        if begins[0] != &binding || reports[0].binding != binding {
            reject(signals, miss, "trust_scope_mismatch");
            continue;
        }
        match reports[0].outcome {
            ActionOutcome::Success => {}
            ActionOutcome::Failure => signals.failed = true,
            ActionOutcome::Cancelled => signals.cancelled = true,
            ActionOutcome::Skipped => signals.not_run = true,
        }
    }
    if request
        .action_begins
        .iter()
        .any(|item| !expected.contains(item.matrix_key.as_str()))
        || request
            .action_reports
            .iter()
            .any(|item| !expected.contains(item.binding.matrix_key.as_str()))
    {
        reject(signals, miss, "cache_corrupt");
    }
}

/// Evidence corruption uses closed final-report tokens.
fn reject(signals: &mut Signals, miss: &mut BTreeSet<String>, token: &str) {
    signals.planning_failed = true;
    miss.insert(token.to_owned());
}

/// Read only action files named by planned entries, preserving separate channels.
pub(crate) fn read_staged_actions(
    raw_plan: &serde_json::Value,
    dir: &Path,
    errors: &mut Vec<String>,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let Ok(plan) = serde_json::from_value::<Plan>(raw_plan.clone()) else {
        return (Vec::new(), Vec::new());
    };
    let mut begins = Vec::new();
    let mut reports = Vec::new();
    for entry in plan
        .matrix
        .include
        .iter()
        .filter(|entry| executes_action(&plan, entry))
    {
        let Some(home) = entry_home(dir, entry, errors) else {
            continue;
        };
        let actions = home.join("actions");
        if path_is_symlink(&actions) {
            errors.push(format!("symlink_action:{}", entry.matrix_key));
            continue;
        }
        read_action_file(
            &actions.join("begin.json"),
            "begin",
            &entry.matrix_key,
            &mut begins,
            errors,
        );
        read_action_file(
            &actions.join("report.json"),
            "report",
            &entry.matrix_key,
            &mut reports,
            errors,
        );
    }
    (begins, reports)
}

/// Select evidence independently of ordinary matrix coverage, refusing ambiguity.
fn entry_home(dir: &Path, entry: &MatrixEntry, errors: &mut Vec<String>) -> Option<PathBuf> {
    if validate_artifact_id(&entry.artifact_id).is_err()
        || validate_matrix_key(&entry.matrix_key).is_err()
    {
        errors.push("invalid_action_path".to_owned());
        return None;
    }
    let artifact = dir.join(&entry.artifact_id);
    let direct = artifact.join(&entry.matrix_key);
    let nested_parent = artifact.join(&entry.artifact_id);
    let nested = nested_parent.join(&entry.matrix_key);
    let selection = (|| {
        directory_shape(&artifact)?;
        directory_shape(&nested_parent)?;
        match (
            action_home_populated(&direct)?,
            action_home_populated(&nested)?,
        ) {
            (true, true) => Err("ambiguous"),
            (false, true) => Ok(nested),
            (_, false) => Ok(direct),
        }
    })();
    match selection {
        Ok(home) => Some(home),
        Err(kind) => {
            errors.push(format!("{kind}_action:{}", entry.matrix_key));
            None
        }
    }
}

/// Inspect directory nodes without following links, including unselected layouts.
fn directory_shape(path: &Path) -> Result<(), &'static str> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err("symlink"),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err("unreadable"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("unreadable"),
    }
}

/// Empty direct directories cannot hide nested begin or terminal evidence.
fn action_home_populated(home: &Path) -> Result<bool, &'static str> {
    directory_shape(home)?;
    let actions = home.join("actions");
    directory_shape(&actions)?;
    let mut populated = false;
    for name in ["begin.json", "report.json"] {
        match std::fs::symlink_metadata(actions.join(name)) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err("symlink"),
            Ok(_) => populated = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("unreadable"),
        }
    }
    Ok(populated)
}

/// Strict JSON, bounded bytes, and no symlink final component.
fn read_action_file(
    path: &Path,
    kind: &str,
    key: &str,
    values: &mut Vec<serde_json::Value>,
    errors: &mut Vec<String>,
) {
    match read_staged_text(path, 16 * 1024) {
        Ok(text) => match parse_strict_json(&text) {
            Ok(value) => values.push(value),
            Err(_) => errors.push(format!("unparsable_action_{kind}:{key}")),
        },
        Err(error) => errors.push(format!("{error}_action_{kind}:{key}")),
    }
}

#[cfg(test)]
#[path = "action_admission_tests.rs"]
mod tests;
