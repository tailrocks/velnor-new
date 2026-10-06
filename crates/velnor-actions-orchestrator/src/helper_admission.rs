//! Exact planned native helper evidence inventory and bounded staged reads.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    HelperObligationBinding, HelperObligationOutcome, HelperObligationReport, MatrixEntry,
    ObligationDecision, Plan, parse_strict_json, validate_artifact_id, validate_matrix_key,
};

use super::MergeRequest;
use crate::cover::Signals;
use crate::retrieve_reports::{path_is_symlink, read_staged_text};

/// Qualified native ownership survives omitted or malformed descriptors.
fn needs_helper(entry: &MatrixEntry) -> bool {
    entry.adapter_metadata.get("helper_obligation").is_some()
        || crate::helper_obligation_binding::requires_helper_obligation(entry)
}

/// Baseline coverage has no helper execution; normal merge validates its proof.
pub(crate) fn executes_helper(plan: &Plan, entry: &MatrixEntry) -> bool {
    needs_helper(entry)
        && plan
            .obligations
            .iter()
            .find(|ob| ob.task_id == entry.task_id)
            .is_none_or(|ob| ob.decision != ObligationDecision::CoveredByTrustedBaseline)
}

/// Derive the binding from the validated plan and closed producer contract.
fn expected_binding(plan: &Plan, entry: &MatrixEntry) -> Option<HelperObligationBinding> {
    let obligation = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == entry.task_id)?;
    if obligation.decision != ObligationDecision::Execute {
        return None;
    }
    let helper = format!("velnor-helper-{}", entry.matrix_key);
    crate::helper_obligation_report::binding(plan, entry, &obligation.task_digest, &helper).ok()
}

/// Prove exact helper inventory and source binding before accepting coverage.
pub(crate) fn check_helpers(
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
        .filter(|entry| executes_helper(plan, entry))
    {
        expected.insert(entry.matrix_key.as_str());
        let Some(binding) = expected_binding(plan, entry) else {
            reject(signals, miss, "cache_corrupt");
            continue;
        };
        check_binding(&binding, request, signals, miss);
    }
    if request
        .helper_begins
        .iter()
        .any(|item| !expected.contains(item.matrix_key.as_str()))
        || request
            .helper_reports
            .iter()
            .any(|item| !expected.contains(item.binding.matrix_key.as_str()))
    {
        reject(signals, miss, "cache_corrupt");
    }
}

/// Compare independent begin and terminal channels against compiled authority.
fn check_binding(
    binding: &HelperObligationBinding,
    request: &MergeRequest,
    signals: &mut Signals,
    miss: &mut BTreeSet<String>,
) {
    let begins: Vec<_> = request
        .helper_begins
        .iter()
        .filter(|begin| begin.matrix_key == binding.matrix_key)
        .collect();
    let reports: Vec<&HelperObligationReport> = request
        .helper_reports
        .iter()
        .filter(|report| report.binding.matrix_key == binding.matrix_key)
        .collect();
    if begins.len() != 1 || reports.len() != 1 {
        reject(signals, miss, "source_missing");
        return;
    }
    if begins[0].validate().is_err() || reports[0].validate().is_err() {
        reject(signals, miss, "cache_corrupt");
        return;
    }
    if begins[0] != binding || &reports[0].binding != binding {
        reject(signals, miss, "trust_scope_mismatch");
        return;
    }
    match reports[0].outcome {
        HelperObligationOutcome::Success => {}
        HelperObligationOutcome::Failure => signals.failed = true,
        HelperObligationOutcome::Cancelled => signals.cancelled = true,
        HelperObligationOutcome::Skipped => signals.not_run = true,
    }
}

/// Evidence corruption uses closed final-report tokens.
fn reject(signals: &mut Signals, miss: &mut BTreeSet<String>, token: &str) {
    signals.planning_failed = true;
    miss.insert(token.to_owned());
}

/// Read only helper files named by planned entries, preserving separate channels.
pub(crate) fn read_staged_helpers(
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
        .filter(|entry| executes_helper(&plan, entry))
    {
        let Some(home) = entry_home(dir, entry, errors) else {
            continue;
        };
        let helpers = home.join("helpers");
        if path_is_symlink(&helpers) {
            errors.push(format!("symlink_helper:{}", entry.matrix_key));
            continue;
        }
        read_helper_file(
            &helpers.join("begin.json"),
            "begin",
            &entry.matrix_key,
            &mut begins,
            errors,
        );
        read_helper_file(
            &helpers.join("report.json"),
            "report",
            &entry.matrix_key,
            &mut reports,
            errors,
        );
    }
    (begins, reports)
}

/// Select exact helper evidence independently of matrix-report availability.
fn entry_home(dir: &Path, entry: &MatrixEntry, errors: &mut Vec<String>) -> Option<PathBuf> {
    if validate_artifact_id(&entry.artifact_id).is_err()
        || validate_matrix_key(&entry.matrix_key).is_err()
    {
        errors.push("invalid_helper_path".to_owned());
        return None;
    }
    let artifact = dir.join(&entry.artifact_id);
    let direct = artifact.join(&entry.matrix_key);
    let nested = artifact.join(&entry.artifact_id);
    let nested_home = nested.join(&entry.matrix_key);
    if path_is_symlink(&artifact) || path_is_symlink(&nested) {
        errors.push(format!("symlink_helper:{}", entry.matrix_key));
        return None;
    }
    let layouts = (
        helper_evidence_exists(&direct),
        helper_evidence_exists(&nested_home),
    );
    match layouts {
        (Ok(true), Ok(true)) => {
            errors.push(format!("ambiguous_helper_layout:{}", entry.matrix_key));
            None
        }
        (Ok(false), Ok(true)) => Some(nested_home),
        (Ok(_), Ok(false)) => Some(direct),
        (Err(kind), _) | (_, Err(kind)) => {
            errors.push(format!("{kind}_helper:{}", entry.matrix_key));
            None
        }
    }
}

/// Empty directories grant no precedence; exact evidence components reject links.
fn helper_evidence_exists(home: &Path) -> Result<bool, &'static str> {
    let helpers = home.join("helpers");
    for directory in [home, helpers.as_path()] {
        match std::fs::symlink_metadata(directory) {
            Ok(meta) if meta.file_type().is_symlink() => return Err("symlink"),
            Ok(meta) if !meta.is_dir() => return Err("unreadable"),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err("unreadable"),
        }
    }
    let mut present = false;
    for name in ["begin.json", "report.json"] {
        match std::fs::symlink_metadata(helpers.join(name)) {
            Ok(meta) if meta.file_type().is_symlink() => return Err("symlink"),
            Ok(_) => present = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("unreadable"),
        }
    }
    Ok(present)
}

/// Strict JSON, bounded bytes, and no symlink final component.
fn read_helper_file(
    path: &Path,
    kind: &str,
    key: &str,
    values: &mut Vec<serde_json::Value>,
    errors: &mut Vec<String>,
) {
    match read_staged_text(path, 1024 * 1024) {
        Ok(text) => match parse_strict_json(&text) {
            Ok(value) => values.push(value),
            Err(_) => errors.push(format!("unparsable_helper_{kind}:{key}")),
        },
        Err(error) => errors.push(format!("{error}_helper_{kind}:{key}")),
    }
}

#[cfg(test)]
#[path = "helper_admission_tests.rs"]
mod tests;
