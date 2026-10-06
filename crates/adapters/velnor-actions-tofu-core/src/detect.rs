//! Tofu detector entry: registered, marker-free by design.
//!
//! The entry exists so the closed `Stack` dispatch covers `tofu` from
//! registration on. It emits no candidates itself: file markers alone
//! never suffice (WEAK `.tf`-only evidence never claims; STRONG
//! evidence without a table only advises). Configured roots map to
//! candidates through [`crate::qualify_roots`], which sees the table.

use velnor_actions_contract_config::VelnorConfig;
use velnor_actions_contract_planning::{DetectedProject, FileIndex, StackCandidate};

/// Discover stack candidates: the tofu detector entry.
///
/// Always empty: tofu units need configured roots before any unit
/// can be named, and this entry observes the index only.
#[must_use]
pub fn discover_stack_candidates(_index: &FileIndex) -> Vec<StackCandidate> {
    debug_assert!(
        VelnorConfig::REGISTERED_STACKS.contains(&crate::STACK_ID),
        "detector emits only registered stacks"
    );
    Vec::new()
}

/// Lift tofu unit candidates to detector records in stable order.
///
/// The caller partitions candidates by stack; every candidate here
/// is the tofu step's own output.
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

/// Evidence path backing a tofu unit root directory.
///
/// Tofu roots are multi-file (no single manifest), so the unit
/// directory itself is the evidence path: empty for the repository
/// root, else the root directory.
#[must_use]
pub fn manifest_for_unit_root(unit_root: &str) -> String {
    unit_root.to_owned()
}
