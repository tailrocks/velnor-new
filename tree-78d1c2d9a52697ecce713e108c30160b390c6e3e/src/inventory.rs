//! Cargo inventory fetching plus locked/offline qualification.

use std::path::{Path, PathBuf};

use velnor_actions_mise::{MetadataDiscovery, MetadataQualification, ToolCatalog};
use velnor_actions_rust::{CandidateOutcome, WorkspaceRecord, parse_metadata_json};

use crate::OrchestratorError;
use crate::decisions::{MetadataFailure, classify_metadata_failure};
use crate::discover::{PlannedWorkspace, workspace_lock, workspace_manifest};

/// Candidate outcomes plus successful manifest inventories.
pub(crate) type Inventories = (Vec<CandidateOutcome>, Vec<(String, WorkspaceRecord)>);

/// Run metadata discovery for every candidate manifest.
pub(crate) fn run_inventories(
    root: &Path,
    candidates: &[velnor_actions_rust::CargoCandidate],
) -> Result<Inventories, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    let mut outcomes = Vec::with_capacity(candidates.len());
    let mut inventories = Vec::new();
    for candidate in candidates {
        let manifest = candidate.manifest.clone();
        match fetch_inventory(root, &manifest, &catalog) {
            Ok(record) => {
                outcomes.push(CandidateOutcome {
                    manifest: manifest.clone(),
                    metadata_ok: true,
                    diagnostic: None,
                });
                inventories.push((manifest, record));
            }
            Err(FetchFailure::Malformed(diagnostic)) => outcomes.push(CandidateOutcome {
                manifest,
                metadata_ok: false,
                diagnostic: Some(diagnostic),
            }),
            Err(FetchFailure::Incomplete(problem)) => {
                return Err(OrchestratorError::PreparationIncomplete { manifest, problem });
            }
        }
    }
    Ok((outcomes, inventories))
}

/// Why one manifest produced no inventory.
enum FetchFailure {
    Malformed(String),
    Incomplete(String),
}

/// Classify a nonzero metadata exit: offline deps abort, malformed selects.
fn classify_exit_failure(stderr: &str) -> FetchFailure {
    match classify_metadata_failure(stderr) {
        MetadataFailure::Incomplete => {
            let first = stderr.lines().next().unwrap_or("metadata_offline").trim();
            let short: String = first.chars().take(160).collect();
            FetchFailure::Incomplete(format!("metadata_offline:{short}"))
        }
        MetadataFailure::Malformed => FetchFailure::Malformed(stderr.to_owned()),
    }
}

/// Discover and parse one manifest through pinned Cargo.
fn fetch_inventory(
    root: &Path,
    manifest: &str,
    catalog: &ToolCatalog,
) -> Result<WorkspaceRecord, FetchFailure> {
    let path: PathBuf = root.join(manifest);
    let request = MetadataDiscovery::new(path)
        .map_err(|err| FetchFailure::Incomplete(format!("bad_manifest_path:{err}")))?;
    let command = request
        .command(catalog)
        .map_err(|err| FetchFailure::Incomplete(err.to_string()))?;
    let auto_install_off = command.disables_auto_install();
    let output = command
        .run()
        .map_err(|err| FetchFailure::Incomplete(err.to_string()))?;
    if !output.success {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if auto_install_off && is_tool_missing(&stderr) {
            return Err(FetchFailure::Incomplete(format!(
                "tool_missing:{}",
                stderr.lines().next().unwrap_or_default().trim()
            )));
        }
        return Err(classify_exit_failure(&stderr));
    }
    let json = output
        .stdout_text("mise")
        .map_err(|err| FetchFailure::Incomplete(err.to_string()))?;
    parse_metadata_json(&json, root, manifest)
        .map_err(|err| FetchFailure::Malformed(err.to_string()))
}

/// True when stderr reports a missing tool rather than a bad manifest.
fn is_tool_missing(stderr: &str) -> bool {
    let text = stderr.to_lowercase();
    [
        "not installed",
        "missing tool",
        "tool missing",
        "no such tool",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

/// Qualify locked/offline resolution wherever a lockfile pins deps.
///
/// A lockfile promises reproducible offline resolution; any qualification
/// failure aborts with `preparation_incomplete` and never fetches.
/// Lockless workspaces have nothing pinned, so nothing to qualify.
pub(crate) fn qualify_workspaces(
    root: &Path,
    workspaces: &[PlannedWorkspace],
) -> Result<(), OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    for workspace in workspaces {
        let prefix = workspace.record.workspace_root.clone();
        let lock = workspace_lock(&prefix);
        if !root.join(&lock).is_file() {
            continue;
        }
        let manifest = workspace_manifest(&prefix);
        let request = MetadataQualification::new(root.join(&manifest)).map_err(|err| {
            OrchestratorError::PreparationIncomplete {
                manifest: manifest.clone(),
                problem: format!("bad_manifest_path:{err}"),
            }
        })?;
        if let Err(err) = request.run(&catalog) {
            let first = err.to_string();
            let short: String = first
                .lines()
                .next()
                .unwrap_or("metadata_offline")
                .chars()
                .take(160)
                .collect();
            return Err(OrchestratorError::PreparationIncomplete {
                manifest,
                problem: format!("metadata_offline:{short}"),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_missing_markers_are_conservative() {
        assert!(is_tool_missing("mise ERROR Tool rust@1.2.3 not installed"));
        assert!(is_tool_missing("No such tool: nextest"));
        assert!(!is_tool_missing(
            "error: failed to parse manifest at Cargo.toml"
        ));
        assert!(!is_tool_missing(""));
    }
}
