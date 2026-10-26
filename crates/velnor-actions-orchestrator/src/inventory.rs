// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use velnor_actions_contract::{CandidateOutcome, Stack, StackCandidate};
use velnor_actions_mise::{MetadataDiscovery, PinnedTool, ToolCatalog, ToolPresence};
use velnor_actions_rust::{WorkspaceRecord, parse_metadata_json};

use crate::OrchestratorError;
use crate::decisions::{MetadataFailure, classify_metadata_failure};
use crate::generate::ToolSnapshot;
use crate::internal::phase_timing::PlanPhaseTimings;
use crate::inventory_reuse::MemberIndex;
use crate::select_tofu::TofuSelectionUnit;
#[path = "inventory_tofu.rs"]
mod tofu;
use self::tofu::analyze_tofu_unit;

pub(crate) type Inventories = (Vec<CandidateOutcome>, Vec<(String, WorkspaceRecord)>);

const MAX_METADATA_LANES: usize = 1;

pub(crate) fn run_inventories(
    root: &Path,
    candidates: &[StackCandidate],
    files: &[String],
    reads: &mut velnor_actions_tofu::FileCache,
    catalog: &ToolCatalog,
) -> Result<(Inventories, Vec<TofuSelectionUnit>), OrchestratorError> {
    run_inventories_inner(root, candidates, files, reads, None, catalog)
}

pub(crate) fn run_inventories_with_phase_timings(
    root: &Path,
    candidates: &[StackCandidate],
    files: &[String],
    reads: &mut velnor_actions_tofu::FileCache,
    phases: &mut PlanPhaseTimings,
    catalog: &ToolCatalog,
) -> Result<(Inventories, Vec<TofuSelectionUnit>), OrchestratorError> {
    run_inventories_inner(root, candidates, files, reads, Some(phases), catalog)
}

fn run_inventories_inner(
    root: &Path,
    candidates: &[StackCandidate],
    files: &[String],
    reads: &mut velnor_actions_tofu::FileCache,
    mut phases: Option<&mut PlanPhaseTimings>,
    catalog: &ToolCatalog,
) -> Result<(Inventories, Vec<TofuSelectionUnit>), OrchestratorError> {
    let mut manifests = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        manifests.push(manifest_for_candidate(candidate)?);
    }
    let mut rust_manifests = Vec::new();
    let mut tofu_units: Vec<&str> = Vec::new();
    for (candidate, manifest) in candidates.iter().zip(manifests.iter()) {
        match Stack::require_known(&candidate.stack_id) {
            Ok(Stack::Rust) => rust_manifests.push(manifest.clone()),
            Ok(Stack::Tofu) => tofu_units.push(candidate.unit_root.as_str()),
            Ok(Stack::Mise) => return Err(explicit_check_candidate_error()),
            Ok(Stack::Native) => return Err(explicit_native_check_candidate_error()),
            Err(err) => {
                return Err(OrchestratorError::Detection {
                    problem: err.to_string(),
                });
            }
        }
    }
    if !rust_manifests.is_empty() {
        require_rust_pin(catalog)?;
    }
    let known: BTreeSet<String> = manifests.iter().cloned().collect();
    let mut load =
        |manifest: &str| fetch_inventory(root, manifest, catalog, &known, phases.as_deref_mut());
    let (rust_outcomes, inventories) = run_with(root, &rust_manifests, true, &mut load)?;
    let mut tofu_outcomes = BTreeMap::new();
    let mut tofu_selection = Vec::new();
    for unit in tofu_units {
        let (outcome, record) = analyze_tofu_unit(root, files, unit, &mut *reads);
        tofu_outcomes.insert(unit.to_owned(), outcome);
        tofu_selection.extend(record);
    }
    tofu_selection.sort_by(|left, right| left.root.cmp(&right.root));
    let mut rust_iter = rust_outcomes.into_iter();
    let mut outcomes = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        match Stack::require_known(&candidate.stack_id) {
            Ok(Stack::Rust) => {
                outcomes.push(rust_iter.next().unwrap_or_else(|| CandidateOutcome {
                    manifest: manifest_for_rust(&candidate.unit_root),
                    metadata_ok: false,
                    diagnostic: Some("missing_outcome".to_owned()),
                }));
            }
            Ok(Stack::Tofu) => {
                outcomes.push(
                    tofu_outcomes
                        .remove(candidate.unit_root.as_str())
                        .unwrap_or(CandidateOutcome {
                            manifest: candidate.unit_root.clone(),
                            metadata_ok: false,
                            diagnostic: Some("missing_outcome".to_owned()),
                        }),
                );
            }
            Ok(Stack::Mise) => return Err(explicit_check_candidate_error()),
            Ok(Stack::Native) => return Err(explicit_native_check_candidate_error()),
            Err(err) => {
                return Err(OrchestratorError::Detection {
                    problem: err.to_string(),
                });
            }
        }
    }
    Ok(((outcomes, inventories), tofu_selection))
}

fn manifest_for_candidate(candidate: &StackCandidate) -> Result<String, OrchestratorError> {
    match Stack::require_known(&candidate.stack_id) {
        Ok(Stack::Rust) => Ok(manifest_for_rust(&candidate.unit_root)),
        Ok(Stack::Tofu) => Ok(velnor_actions_tofu::manifest_for_unit_root(
            &candidate.unit_root,
        )),
        Ok(Stack::Mise) => Err(explicit_check_candidate_error()),
        Ok(Stack::Native) => Err(explicit_native_check_candidate_error()),
        Err(err) => Err(OrchestratorError::Detection {
            problem: err.to_string(),
        }),
    }
}

fn explicit_check_candidate_error() -> OrchestratorError {
    OrchestratorError::Detection {
        problem: "mise_checks_are_explicit".to_owned(),
    }
}

fn explicit_native_check_candidate_error() -> OrchestratorError {
    OrchestratorError::Detection {
        problem: "native_checks_are_explicit".to_owned(),
    }
}

fn manifest_for_rust(unit_root: &str) -> String {
    velnor_actions_rust::manifest_for_unit_root(unit_root)
}

fn run_with(
    root: &Path,
    manifests: &[String],
    reuse: bool,
    mut load: impl FnMut(&str) -> Result<WorkspaceRecord, FetchFailure>,
) -> Result<Inventories, OrchestratorError> {
    let tools = ToolSnapshot::capture(root);
    let mut outcomes = Vec::with_capacity(manifests.len());
    let mut inventories = Vec::new();
    let mut index = MemberIndex::default();
    for lane in manifests.chunks(MAX_METADATA_LANES) {
        for manifest in lane {
            let manifest = manifest.clone();
            if reuse && let Some(record) = index.reuse_for(root, &manifest, &inventories) {
                outcomes.push(ok_outcome(manifest.clone()));
                inventories.push((manifest, record));
                continue;
            }
            match load(&manifest) {
                Ok(record) => {
                    outcomes.push(ok_outcome(manifest.clone()));
                    index.insert(inventories.len(), &record);
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
    }
    tools.verify(root)?;
    Ok((outcomes, inventories))
}

fn ok_outcome(manifest: String) -> CandidateOutcome {
    CandidateOutcome {
        manifest,
        metadata_ok: true,
        diagnostic: None,
    }
}

enum FetchFailure {
    Malformed(String),
    Incomplete(String),
}

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

#[expect(
    clippy::needless_option_as_deref,
    reason = "the Option is intentionally reborrowed multiple times"
)]
fn fetch_inventory(
    root: &Path,
    manifest: &str,
    catalog: &ToolCatalog,
    known: &BTreeSet<String>,
    mut phases: Option<&mut PlanPhaseTimings>,
) -> Result<WorkspaceRecord, FetchFailure> {
    let path: PathBuf = root.join(manifest);
    let request = MetadataDiscovery::new(path)
        .map_err(|err| FetchFailure::Incomplete(format!("bad_manifest_path:{err}")))?;
    let command = request
        .command(catalog)
        .map_err(|err| FetchFailure::Incomplete(err.to_string()))?;
    let auto_install_off = command.disables_auto_install();
    if let Some(phases) = phases.as_deref_mut() {
        phases.metadata_commands += 1;
    }
    let started = phases.as_ref().map(|_| Instant::now());
    let output = command.run();
    if let (Some(phases), Some(started)) = (phases.as_deref_mut(), started) {
        phases.metadata_run_us += started.elapsed().as_micros();
    }
    let output = output.map_err(|err| FetchFailure::Incomplete(err.to_string()))?;
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
    let started = phases.as_ref().map(|_| Instant::now());
    let result = match output.stdout_text("mise") {
        Ok(json) => parse_metadata_json(&json, root, manifest, known)
            .map_err(|err| FetchFailure::Malformed(err.to_string())),
        Err(err) => Err(FetchFailure::Incomplete(err.to_string())),
    };
    if let (Some(phases), Some(started)) = (phases.as_deref_mut(), started) {
        phases.metadata_parse_us += started.elapsed().as_micros();
    }
    result
}

fn is_tool_missing(stderr: &str) -> bool {
    let text = stderr.to_lowercase();
    text.contains("not installed")
        || text.contains("missing tool")
        || text.contains("tool missing")
        || text.contains("no such tool")
}

pub(crate) fn require_rust_pin(catalog: &ToolCatalog) -> Result<(), OrchestratorError> {
    let presence =
        ToolPresence::new(vec![PinnedTool::Rust]).map_err(|err| OrchestratorError::Inventory {
            problem: err.to_string(),
        })?;
    presence
        .run_with_observation(catalog)
        .map_err(|failure| OrchestratorError::Inventory {
            problem: match failure.observation {
                Some(observation) => format!(
                    "{}; mise_ls_observation={}",
                    failure.error,
                    observation.to_json_line()
                ),
                None => failure.error.to_string(),
            },
        })
}

#[path = "inventory_resolve.rs"]
mod resolve;
pub(crate) use resolve::{WorkspaceResolve, qualify_workspaces};
