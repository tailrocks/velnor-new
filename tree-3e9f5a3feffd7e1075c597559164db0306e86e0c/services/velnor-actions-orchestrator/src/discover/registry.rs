//! Closed detector dispatch; explicit checks bypass detector inventories.
use crate::OrchestratorError;
use velnor_actions_contract::{
    DETECTION_SCHEMA, DetectedProject, DetectorEntry, Stack, StackCandidate,
};

/// Detector registry (stack id, record schema, implementation), ascending.
pub(super) const DETECTORS: [DetectorEntry; 2] = [
    (
        Stack::Rust.id(),
        DETECTION_SCHEMA,
        velnor_actions_rust::discover_stack_candidates,
    ),
    (
        Stack::Tofu.id(),
        DETECTION_SCHEMA,
        velnor_actions_tofu_core::discover_stack_candidates,
    ),
];

/// Convert neutral candidates to detected projects via closed dispatch.
pub(super) fn detected_projects(
    candidates: &[StackCandidate],
) -> Result<Vec<DetectedProject>, OrchestratorError> {
    let mut rust = Vec::new();
    let mut tofu = Vec::new();
    let mut order: Vec<Stack> = Vec::new();
    for candidate in candidates {
        match Stack::require_known(&candidate.stack_id) {
            Ok(stack @ Stack::Rust) => {
                if !order.contains(&stack) {
                    order.push(stack);
                }
                rust.push(candidate.clone());
            }
            Ok(stack @ Stack::Tofu) => {
                if !order.contains(&stack) {
                    order.push(stack);
                }
                tofu.push(candidate.clone());
            }
            Ok(Stack::Mise) => {
                return Err(OrchestratorError::Detection {
                    problem: "mise_checks_are_explicit".to_owned(),
                });
            }
            Err(err) => {
                return Err(OrchestratorError::Detection {
                    problem: err.to_string(),
                });
            }
        }
    }
    let rust_projects = velnor_actions_rust::detected_projects_for_units(&rust);
    let tofu_projects = velnor_actions_tofu_core::detected_projects_for_units(&tofu);
    let mut projects = Vec::with_capacity(rust_projects.len() + tofu_projects.len());
    for stack in order {
        match stack {
            Stack::Rust => projects.extend(rust_projects.clone()),
            Stack::Tofu => projects.extend(tofu_projects.clone()),
            Stack::Mise => {
                return Err(OrchestratorError::Detection {
                    problem: "mise_checks_are_explicit".to_owned(),
                });
            }
        }
    }
    Ok(projects)
}

/// Registered detectors as (stack ID, record schema), ascending.
pub(crate) fn detector_entries() -> Vec<(&'static str, u32)> {
    DETECTORS
        .iter()
        .map(|(stack_id, schema, _)| (*stack_id, *schema))
        .collect()
}
