// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{DetectedProject, FileIndex, StackCandidate, VelnorConfig};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CargoCandidate {
    pub manifest: String,
}

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

fn is_manifest(path: &str) -> bool {
    path == "Cargo.toml" || path.ends_with("/Cargo.toml")
}

#[must_use]
pub fn project_root_for_manifest(manifest: &str) -> String {
    manifest
        .rsplit_once('/')
        .map_or_else(String::new, |(dir, _)| dir.to_owned())
}

#[must_use]
pub fn discover_stack_candidates(index: &FileIndex) -> Vec<StackCandidate> {
    debug_assert!(
        VelnorConfig::REGISTERED_STACKS.contains(&crate::STACK_ID),
        "detector emits only registered stacks"
    );
    discover_candidates(index)
        .iter()
        .map(|candidate| StackCandidate {
            stack_id: crate::STACK_ID.to_owned(),
            unit_root: project_root_for_manifest(&candidate.manifest),
        })
        .collect()
}

#[must_use]
pub fn detected_projects_for_units(candidates: &[StackCandidate]) -> Vec<DetectedProject> {
    candidates
        .iter()
        .map(|candidate| DetectedProject {
            stack_id: crate::STACK_ID.to_owned(),
            project_root: candidate.unit_root.clone(),
            manifest: manifest_for_unit_root(&candidate.unit_root),
        })
        .collect()
}

#[must_use]
pub fn manifest_for_key(key: &str) -> String {
    if key == "root" {
        "Cargo.toml".to_owned()
    } else {
        format!("{key}/Cargo.toml")
    }
}

#[must_use]
pub fn manifest_for_unit_root(unit_root: &str) -> String {
    if unit_root.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{unit_root}/Cargo.toml")
    }
}
