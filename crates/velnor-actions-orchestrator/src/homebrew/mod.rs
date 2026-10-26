// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "../homebrew_adoption.rs"]
pub(crate) mod adoption;
#[path = "../homebrew_adoption_authoring.rs"]
pub(crate) mod authoring;
#[path = "../homebrew_authority.rs"]
pub(crate) mod authority;
#[path = "../homebrew_command.rs"]
pub(crate) mod command;
#[path = "../homebrew_hosted_profile.rs"]
pub(crate) mod hosted_profile;
#[path = "../homebrew_install.rs"]
pub(crate) mod install;
#[path = "../homebrew_qualification.rs"]
pub(crate) mod qualification;
#[path = "../native_check_runtime.rs"]
pub(crate) mod runtime;
#[path = "../homebrew_adoption_sdk_binding.rs"]
pub(crate) mod sdk_binding;
pub(crate) mod source_policy;
#[path = "../homebrew_structural_cutover.rs"]
pub(crate) mod structural_cutover;

#[path = "../tap_tree_validation.rs"]
pub(crate) mod tree_validation;
