//! Debian/ELF packaging, signed APT feeds and immutable artifact transport sources.
//!
//! This module owns the complete fixed Python source closure. Orchestration
//! composes protected jobs; Mise binds every source before launching helpers.

mod environment;
mod helpers;
mod standalone;

use velnor_actions_contract::ContractError;

use crate::{OwnedSupportFile, SupportBundle};

pub use helpers::{AptOperation, HELPER_PATHS, compiled_helper, compiled_pages_admission};

/// Complete fixed APT support paths. Every companion belongs to the closure.
pub const SUPPORT_PATHS: &[&str] = &[
    ".github/velnor/apt_delivery.py",
    ".github/velnor/delivery_apt_transport.py",
];

/// Produce the complete compiled APT source closure without executing commands.
///
/// # Errors
/// Rejects invalid generator versions or malformed compiled source records.
pub fn support_sources(version: &str) -> Result<SupportBundle, ContractError> {
    let entry = standalone::standalone_body(version)?;
    SupportBundle::compiled(vec![
        OwnedSupportFile::compiled(SUPPORT_PATHS[0], &entry, version)?,
        OwnedSupportFile::compiled(
            SUPPORT_PATHS[1],
            include_str!("delivery_apt_transport.py"),
            version,
        )?,
    ])
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
