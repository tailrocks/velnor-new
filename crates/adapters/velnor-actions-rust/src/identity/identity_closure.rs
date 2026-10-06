//! Extension-input resolution states and verified construction (P03).
//!
//! Declared via `#[path]` from `identity.rs` (no `lib.rs` edit).
//! Reports which semantic inputs an extension leaves unresolved
//! (`None`/empty is unknown, never proven absent) and validates
//! identity paths before construction.

use velnor_actions_contract::{ContractError, normalize_posix_path};

use super::GroupExtensionInputs;
use crate::task_identity::RustTaskIdentityExtension;
use crate::tasks::TaskGroup;

/// One semantic input an extension leaves unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedInput {
    /// Lockfile digest unknown.
    Lockfile,
    /// Nextest-config digest unknown for a Nextest task.
    NextestConfig,
    /// Archive source unknown for a Nextest `Build` task.
    ArchiveSource,
    /// Build-script `rerun-if-changed` inputs unknown.
    RerunInputs,
}

impl UnresolvedInput {
    /// Stable vocabulary word for error codes and logs.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lockfile => "lockfile",
            Self::NextestConfig => "nextest_config",
            Self::ArchiveSource => "archive_source",
            Self::RerunInputs => "rerun_inputs",
        }
    }

    /// Whether this input blocks reuse and coverage decisions.
    ///
    /// Only [`UnresolvedInput::ArchiveSource`] reports without
    /// blocking: production never populates `archive_source`
    /// (`ExtensionBundle::inputs` hardcodes `None`) and binds
    /// archives through the dedicated
    /// `check_archive_identity_with_source` gate instead, so
    /// treating it as blocking here would refuse every Nextest
    /// `Build` unconditionally.
    #[must_use]
    pub fn blocks_gates(&self) -> bool {
        !matches!(self, Self::ArchiveSource)
    }
}

impl RustTaskIdentityExtension {
    /// First gate-blocking unresolved input, in stable inventory order.
    ///
    /// The single enforcement point behind [`unresolved_inputs`]:
    /// reuse and coverage gates call this instead of reimplementing
    /// the slot checks, so the inventory and the gates cannot drift.
    /// [`UnresolvedInput::blocks_gates`] names the blocking subset.
    #[must_use]
    pub fn first_blocking_input(&self) -> Option<UnresolvedInput> {
        unresolved_inputs(self)
            .into_iter()
            .find(UnresolvedInput::blocks_gates)
    }
}

/// Semantic inputs `ext` leaves unresolved, in stable order.
///
/// Unknown means unobserved: the orchestrator resolves each against
/// the checkout (content digest or proven absence) before reuse or
/// coverage may proceed.
#[must_use]
pub fn unresolved_inputs(ext: &RustTaskIdentityExtension) -> Vec<UnresolvedInput> {
    let mut unresolved = Vec::new();
    if ext.lock_slot.is_unknown() {
        unresolved.push(UnresolvedInput::Lockfile);
    }
    if ext.task_kind == crate::tasks::TaskKind::Nextest && ext.nextest_slot.is_unknown() {
        unresolved.push(UnresolvedInput::NextestConfig);
    }
    if ext.task_kind == crate::tasks::TaskKind::Build
        && ext.test_runner == velnor_actions_rust_core::profile::TestRunner::CargoNextest
        && ext.archive.is_none()
    {
        unresolved.push(UnresolvedInput::ArchiveSource);
    }
    if ext.undeclared_reads {
        unresolved.push(UnresolvedInput::RerunInputs);
    }
    unresolved
}

/// Normalize one identity path: repo-relative, explicit rejects.
///
/// Case and Unicode pass through byte-for-byte; empty, absolute,
/// traversing, backslash, and NUL/control-carrying paths fail.
///
/// # Errors
///
/// Returns [`ContractError`] for malformed checkout paths.
pub fn normalize_identity_path(path: &str) -> Result<String, ContractError> {
    if path.is_empty() {
        return Err(ContractError::identity("path", "empty_path"));
    }
    if path.contains('\0') || path.chars().any(char::is_control) {
        return Err(ContractError::identity("path", "control_characters"));
    }
    if path.contains('\\') {
        return Err(ContractError::identity("path", "backslash_separator"));
    }
    normalize_posix_path(path)
}

impl TaskGroup {
    /// Derive the identity extension, validating every identity path.
    ///
    /// The manifest, declared inputs, and rerun inputs must be
    /// well-formed checkout paths; anything else fails instead of
    /// entering the identity preimage silently.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for malformed identity paths.
    pub fn identity_extension_verified(
        &self,
        inputs: &GroupExtensionInputs<'_>,
    ) -> Result<RustTaskIdentityExtension, ContractError> {
        normalize_identity_path(inputs.manifest)?;
        for path in self.declared_inputs.iter().chain(inputs.targets.iter()) {
            normalize_identity_path(path)?;
        }
        if let Some(rerun) = inputs.rerun_inputs {
            for path in rerun {
                normalize_identity_path(path)?;
            }
        }
        Ok(self.identity_extension(inputs))
    }
}

#[cfg(test)]
mod tests;
