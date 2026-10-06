//! Structural `ToFu` inventory analysis extracted from Cargo inventory flow.

use std::path::Path;

use velnor_actions_contract::CandidateOutcome;

use crate::safe_read::read_repo_file_cached;
use crate::select_tofu::TofuSelectionUnit;

use super::ok_outcome;

/// Analyze one tofu unit: bounded reads over the effective set.
///
/// Structural failures (including unreadable, oversize, symlinked,
/// or non-UTF-8 effective files) report a malformed outcome naming
/// the file; module-boundary failures (escapes, missing targets,
/// cycles) likewise report malformed. Findings pass through.
/// Successful units also return their selection record.
pub(super) fn analyze_tofu_unit(
    root: &Path,
    files: &[String],
    unit: &str,
    reads: &mut velnor_actions_tofu::FileCache,
) -> (CandidateOutcome, Option<TofuSelectionUnit>) {
    let failed = |outcome: CandidateOutcome| (outcome, None);
    let selected = match velnor_actions_tofu::files_for_prefix(files, unit) {
        Ok(selected) => selected,
        Err(err) => return failed(malformed_outcome(unit, err.to_string())),
    };
    let effective = velnor_actions_tofu::effective_set(&selected);
    let mut pairs = Vec::with_capacity(effective.len());
    for path in &effective {
        match read_repo_file_cached(root, path, velnor_actions_tofu::MAX_FILE_BYTES, reads) {
            velnor_actions_tofu::PinnedOutcome::Text(text) => pairs.push((path.clone(), text)),
            velnor_actions_tofu::PinnedOutcome::Absent => {
                return failed(malformed_outcome(path, "absent_after_index".to_owned()));
            }
            velnor_actions_tofu::PinnedOutcome::Unreadable(problem) => {
                return failed(malformed_outcome(path, problem));
            }
        }
    }
    match velnor_actions_tofu::analyze_files(&pairs) {
        Ok(unit_record) => {
            match velnor_actions_tofu::qualify_module_edges(root, files, &unit_record.modules) {
                Ok(edges) => {
                    let record = TofuSelectionUnit {
                        root: unit.to_owned(),
                        files: config_files(&selected),
                        edges,
                    };
                    (ok_outcome(unit.to_owned()), Some(record))
                }
                Err(err) => failed(module_error_outcome(&err, unit)),
            }
        }
        Err(err) => {
            let path = unit_error_path(&err, unit);
            failed(malformed_outcome(&path, err.to_string()))
        }
    }
}

/// Sorted config-family files among unit paths (base-text candidates).
fn config_files(selected: &[String]) -> Vec<String> {
    let mut configs: Vec<String> = selected
        .iter()
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            matches!(
                velnor_actions_tofu::family_of(name),
                velnor_actions_tofu::Family::Config | velnor_actions_tofu::Family::Override
            )
        })
        .cloned()
        .collect();
    configs.sort();
    configs
}

/// Evidence path naming a tofu unit failure.
fn unit_error_path(err: &velnor_actions_tofu::UnitError, unit: &str) -> String {
    match err {
        velnor_actions_tofu::UnitError::TooManyFiles { .. } => unit.to_owned(),
        velnor_actions_tofu::UnitError::Parse { path, .. }
        | velnor_actions_tofu::UnitError::UnknownBlock { path, .. }
        | velnor_actions_tofu::UnitError::Shape { path, .. } => path.clone(),
        velnor_actions_tofu::UnitError::Duplicate { second, .. } => second.clone(),
    }
}

/// Malformed outcome for one module-boundary failure.
fn module_error_outcome(err: &velnor_actions_tofu::ModuleError, unit: &str) -> CandidateOutcome {
    let path = match err {
        velnor_actions_tofu::ModuleError::Escape { target }
        | velnor_actions_tofu::ModuleError::MissingTarget { target }
        | velnor_actions_tofu::ModuleError::Unreadable { target }
            if !target.is_empty() =>
        {
            target.clone()
        }
        _ => unit.to_owned(),
    };
    malformed_outcome(&path, err.to_string())
}

/// Malformed outcome for one evidence path and diagnostic.
fn malformed_outcome(path: &str, diagnostic: String) -> CandidateOutcome {
    CandidateOutcome {
        manifest: path.to_owned(),
        metadata_ok: false,
        diagnostic: Some(diagnostic),
    }
}
