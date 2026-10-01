//! Tofu detector entry: registered, marker-free by design.
//!
//! The entry exists so the closed `Stack` dispatch covers `tofu` from
//! registration on. It emits no candidates itself: file markers alone
//! never suffice (WEAK `.tf`-only evidence never claims; STRONG
//! evidence without a table only advises). Configured roots map to
//! candidates through [`crate::qualify_roots`], which sees the table.

use velnor_actions_contract::{FileIndex, StackCandidate, VelnorConfig};

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
