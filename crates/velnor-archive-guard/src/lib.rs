// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
pub mod golden_collector;
pub mod owned_archive_guard;
pub use owned_archive_guard::{
    validate_qualification_manifest, validate_qualification_output_bindings,
};

#[path = "../build_support/archive_guard_inputs.rs"]
mod archive_guard_inputs;

pub fn source_fingerprint(root: &std::path::Path) -> Result<String, String> {
    archive_guard_inputs::fingerprint(root)
        .map(|(fingerprint, _)| fingerprint)
        .map_err(|error| format!("cannot verify archive guard source tree: {error}"))
}
