// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const IGNORED_REASON: &str = "stack_ignored";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedProject {
    pub stack_id: String,

    pub project_root: String,

    pub manifest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectionStatus {
    Selected(DetectedProject),

    Ignored {
        project: DetectedProject,

        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectError {
    DuplicateProject {
        stack_id: String,

        project_root: String,
    },

    MalformedUnit {
        unit: String,

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
