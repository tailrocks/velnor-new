// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "homebrew_structural_cutover/base.rs"]
mod base;
#[path = "homebrew_structural_cutover/candidate.rs"]
mod candidate;

pub(super) use base::protected_transition;
pub(crate) use base::{
    HomebrewSeedOutputBaseIdentity, PrivateVerifiedSeedOutputBase, verify_seed_output_base,
};
pub(crate) use candidate::{
    RegularTree, expected_structural_output_tree, is_exact_legacy_stable_formula,
    validate_structural_candidate,
};
