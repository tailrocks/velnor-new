// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{DetectedProject, FileIndex, StackCandidate, VelnorConfig};

#[must_use]
pub fn discover_stack_candidates(_index: &FileIndex) -> Vec<StackCandidate> {
    debug_assert!(
        VelnorConfig::REGISTERED_STACKS.contains(&crate::STACK_ID),
        "detector emits only registered stacks"
    );
    Vec::new()
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
pub fn manifest_for_unit_root(unit_root: &str) -> String {
    unit_root.to_owned()
}
