//! Tofu detector entry: registered but candidate-free until T09.
//!
//! The entry exists so the closed `Stack` dispatch covers `tofu` from
//! registration on; detection behavior lands with the T09 roots work.

use velnor_actions_contract::{FileIndex, StackCandidate, VelnorConfig};

/// Discover stack candidates: the tofu detector entry.
///
/// Always empty until T09: tofu units need configured roots before
/// the detector can name them.
#[must_use]
pub fn discover_stack_candidates(_index: &FileIndex) -> Vec<StackCandidate> {
    debug_assert!(
        VelnorConfig::REGISTERED_STACKS.contains(&crate::STACK_ID),
        "detector emits only registered stacks"
    );
    Vec::new()
}
