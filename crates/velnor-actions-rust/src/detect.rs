//! Rust detector: `Cargo.toml` candidates, registry order, ignore-after.
//!
//! Detectors observe the post-exclusion index; stack ignores apply after
//! detection completes and never suppress malformed-manifest errors.

use std::collections::BTreeSet;
use std::fmt;

use crate::index::FileIndex;

/// Stack ids with a registered detector, in invocation order.
pub const REGISTERED_STACKS: &[&str] = &["rust"];

/// Reason recorded on ignored detections.
pub const IGNORED_REASON: &str = "stack_ignored";

/// One discovered `Cargo.toml` manifest (post-exclusion).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CargoCandidate {
    /// Repository-relative POSIX path of the manifest.
    pub manifest: String,
}

/// One detector record: stack plus project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedProject {
    /// Registered stack id (`rust`).
    pub stack_id: String,
    /// Repository-relative project directory; empty for the repository root.
    pub project_root: String,
    /// Repository-relative manifest path backing this record.
    pub manifest: String,
}

/// Post-detection selection state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectionStatus {
    /// Detection retained for planning.
    Selected(DetectedProject),
    /// Detection retained as ignored; produces no tasks.
    Ignored {
        /// Ignored detection record.
        project: DetectedProject,
        /// Machine-readable reason.
        reason: String,
    },
}

/// Outcome of the orchestrator's metadata request for one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateOutcome {
    /// Candidate manifest this outcome belongs to.
    pub manifest: String,
    /// Whether metadata parsing succeeded.
    pub metadata_ok: bool,
    /// Cargo diagnostic when `metadata_ok` is false.
    pub diagnostic: Option<String>,
}

/// Detector failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectError {
    /// Two records share one `(stack_id, project_root)` key.
    DuplicateProject {
        /// Duplicated stack id.
        stack_id: String,
        /// Duplicated project root.
        project_root: String,
    },
    /// A discovered manifest is malformed.
    MalformedManifest {
        /// Offending manifest path.
        manifest: String,
        /// Cargo diagnostic.
        diagnostic: String,
    },
}

impl fmt::Display for DetectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateProject {
                stack_id,
                project_root,
            } => write!(f, "duplicate_project:{stack_id}:{project_root}"),
            Self::MalformedManifest {
                manifest,
                diagnostic,
            } => write!(f, "malformed_manifest:{manifest}: {diagnostic}"),
        }
    }
}

impl std::error::Error for DetectError {}

/// Discover every `Cargo.toml` marker exactly once, in sorted order.
#[must_use]
pub fn discover_candidates(index: &FileIndex) -> Vec<CargoCandidate> {
    index
        .files()
        .iter()
        .filter(|path| is_manifest(path))
        .map(|path| CargoCandidate {
            manifest: path.clone(),
        })
        .collect()
}

/// Whether `path` is a `Cargo.toml` marker.
fn is_manifest(path: &str) -> bool {
    path == "Cargo.toml" || path.ends_with("/Cargo.toml")
}

/// Directory holding `manifest`; empty for the repository root.
#[must_use]
pub fn project_root_for_manifest(manifest: &str) -> String {
    manifest
        .rsplit_once('/')
        .map_or_else(String::new, |(dir, _)| dir.to_owned())
}

/// Lift candidates to detector records in stable order.
#[must_use]
pub fn to_detected_projects(candidates: &[CargoCandidate]) -> Vec<DetectedProject> {
    candidates
        .iter()
        .map(|candidate| DetectedProject {
            stack_id: crate::STACK_ID.to_owned(),
            project_root: project_root_for_manifest(&candidate.manifest),
            manifest: candidate.manifest.clone(),
        })
        .collect()
}

/// Reject duplicate `(stack_id, project_root)` records.
///
/// # Errors
///
/// Returns [`DetectError::DuplicateProject`] on the first duplicate key.
pub fn check_duplicates(projects: &[DetectedProject]) -> Result<(), DetectError> {
    let mut seen = BTreeSet::new();
    for project in projects {
        let key = (project.stack_id.clone(), project.project_root.clone());
        if !seen.insert(key.clone()) {
            return Err(DetectError::DuplicateProject {
                stack_id: key.0,
                project_root: key.1,
            });
        }
    }
    Ok(())
}

/// Apply `[stacks].ignore` after detection completes.
///
/// Ignored detections are retained with [`IGNORED_REASON`] and produce no
/// tasks; unknown ids are left to configuration validation.
#[must_use]
pub fn apply_stack_ignores(
    projects: Vec<DetectedProject>,
    ignore: &[String],
) -> Vec<DetectionStatus> {
    projects
        .into_iter()
        .map(|project| {
            if ignore.iter().any(|id| id == &project.stack_id) {
                DetectionStatus::Ignored {
                    project,
                    reason: IGNORED_REASON.to_owned(),
                }
            } else {
                DetectionStatus::Selected(project)
            }
        })
        .collect()
}

/// Borrow the retained detections, skipping ignored records.
#[must_use]
pub fn selected_projects(statuses: &[DetectionStatus]) -> Vec<&DetectedProject> {
    statuses
        .iter()
        .filter_map(|status| match status {
            DetectionStatus::Selected(project) => Some(project),
            DetectionStatus::Ignored { .. } => None,
        })
        .collect()
}

/// Fail on malformed manifests even when their stack is ignored.
///
/// # Errors
///
/// Returns [`DetectError::MalformedManifest`] for the first outcome whose
/// metadata request failed, regardless of selection state.
pub fn check_candidate_outcomes(
    statuses: Vec<DetectionStatus>,
    outcomes: &[CandidateOutcome],
) -> Result<Vec<DetectionStatus>, DetectError> {
    for outcome in outcomes {
        if !outcome.metadata_ok {
            return Err(DetectError::MalformedManifest {
                manifest: outcome.manifest.clone(),
                diagnostic: outcome
                    .diagnostic
                    .clone()
                    .unwrap_or_else(|| "metadata_failed".to_owned()),
            });
        }
    }
    Ok(statuses)
}
