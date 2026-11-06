// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "generate_preserve/in_place.rs"]
mod in_place;
#[path = "generate_preserve/owned.rs"]
mod owned;
#[path = "generate_preserve/preview.rs"]
mod preview;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use in_place::stage_in_place;
pub(super) use owned::{
    generated_marked_dco, generated_marked_script, generated_shared_action, generator_owned,
    retired_marked_lint_config,
};
pub(super) use preview::write_preview;
