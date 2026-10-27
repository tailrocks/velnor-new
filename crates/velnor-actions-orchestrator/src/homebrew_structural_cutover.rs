// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "homebrew_cutover_author_policy.rs"]
mod author_policy;
#[path = "homebrew_structural_cutover/base.rs"]
mod base;
#[path = "homebrew_structural_cutover/candidate.rs"]
mod candidate;
#[path = "homebrew_cutover_marker.rs"]
mod marker;

pub(crate) use author_policy::{require_author, verify_transition as verify_author_policy};
pub(crate) use marker::{CutoverMarker, MARKER_LIMIT, MARKER_PATH, parse_marker, sha256};

pub(crate) use base::protected_transition;
pub(crate) use base::{
    HomebrewSeedOutputBaseIdentity, PrivateVerifiedSeedOutputBase, verify_seed_output_base,
};
pub(crate) use candidate::{
    RegularTree, expected_structural_output_tree, is_exact_legacy_stable_formula,
    validate_structural_candidate,
};
