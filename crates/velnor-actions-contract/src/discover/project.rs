//! Neutral detection records: projects, selection, duplicates.
//!
//! Detectors observe the post-exclusion index; stack ignores apply after
//! detection completes and never suppress malformed-unit errors. The
//! `manifest` field names the evidence path backing the record (a Cargo
//! manifest for Rust; other stacks store their own evidence path there).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Reason recorded on ignored detections.
pub const IGNORED_REASON: &str = "stack_ignored";

/// One detector record: stack plus project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedProject {
    /// Registered stack id (`rust`).
    pub stack_id: String,
    /// Repository-relative project directory; empty for the repository root.
    pub project_root: String,
    /// Repository-relative evidence path backing this record.
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
    /// A discovered unit is malformed.
    MalformedUnit {
        /// Offending evidence path.
        unit: String,
        /// Adapter diagnostic.
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
            Self::MalformedUnit { unit, diagnostic } => {
                write!(f, "malformed_manifest:{unit}: {diagnostic}")
            }
        }
    }
}

impl std::error::Error for DetectError {}

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

/// Reverse dependents of `changed` keys using base plus head edges.
///
/// The union of both graphs is considered so added, removed, or renamed
/// edges cannot hide consumers. Callers project domain edges to
/// `(from, to)` key pairs; this function orders keys only.
#[must_use]
pub fn reverse_closure<N: Ord + Clone>(
    base: &[(N, N)],
    head: &[(N, N)],
    changed: &BTreeSet<N>,
) -> BTreeSet<N> {
    let mut reverse: BTreeMap<&N, Vec<&N>> = BTreeMap::new();
    for (from, to) in base.iter().chain(head.iter()) {
        reverse.entry(to).or_default().push(from);
    }
    let mut selected = changed.clone();
    let mut stack: Vec<&N> = changed.iter().collect();
    while let Some(id) = stack.pop() {
        if let Some(dependents) = reverse.get(id) {
            for dependent in dependents {
                if selected.insert((*dependent).clone()) {
                    stack.push(dependent);
                }
            }
        }
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Malformed units keep the machine-readable token byte-identical.
    #[test]
    fn malformed_unit_keeps_wire_token() {
        let err = DetectError::MalformedUnit {
            unit: "Cargo.toml".to_owned(),
            diagnostic: "bad toml".to_owned(),
        };
        assert_eq!(err.to_string(), "malformed_manifest:Cargo.toml: bad toml");
        let err = DetectError::DuplicateProject {
            stack_id: "rust".to_owned(),
            project_root: String::new(),
        };
        assert_eq!(err.to_string(), "duplicate_project:rust:");
    }

    /// Ignores apply after detection; selection skips ignored records.
    #[test]
    fn ignores_retain_with_reason() {
        let projects = vec![
            DetectedProject {
                stack_id: "rust".to_owned(),
                project_root: String::new(),
                manifest: "Cargo.toml".to_owned(),
            },
            DetectedProject {
                stack_id: "rust".to_owned(),
                project_root: "crates/a".to_owned(),
                manifest: "crates/a/Cargo.toml".to_owned(),
            },
        ];
        assert!(check_duplicates(&projects).is_ok());
        assert!(check_duplicates(&projects[..1]).is_ok());
        let doubled = vec![projects[0].clone(), projects[0].clone()];
        assert!(check_duplicates(&doubled).is_err());
        let statuses = apply_stack_ignores(projects, &["rust".to_owned()]);
        assert_eq!(selected_projects(&statuses).len(), 0);
        assert!(statuses.iter().all(|status| matches!(
            status,
            DetectionStatus::Ignored { reason, .. } if reason == IGNORED_REASON
        )));
    }

    /// The generic closure matches the local-edge behavior over pairs.
    #[test]
    fn closure_selects_base_and_head_consumers() {
        let base = vec![("a".to_owned(), "b".to_owned())];
        let head: Vec<(String, String)> = Vec::new();
        let changed = BTreeSet::from(["b".to_owned()]);
        assert_eq!(
            reverse_closure(&base, &head, &changed),
            BTreeSet::from(["a".to_owned(), "b".to_owned()])
        );
        assert_eq!(
            reverse_closure(&head, &head, &changed),
            BTreeSet::from(["b".to_owned()])
        );
    }
}
