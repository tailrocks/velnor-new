// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{ContractError, normalize_posix_path};

use super::GroupExtensionInputs;
use crate::task_identity::RustTaskIdentityExtension;
use crate::tasks::TaskGroup;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedInput {
    Lockfile,
    NextestConfig,
    ArchiveSource,
    RerunInputs,
}

impl UnresolvedInput {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lockfile => "lockfile",
            Self::NextestConfig => "nextest_config",
            Self::ArchiveSource => "archive_source",
            Self::RerunInputs => "rerun_inputs",
        }
    }

    #[must_use]
    pub fn blocks_gates(&self) -> bool {
        !matches!(self, Self::ArchiveSource)
    }
}

impl RustTaskIdentityExtension {
    #[must_use]
    pub fn first_blocking_input(&self) -> Option<UnresolvedInput> {
        unresolved_inputs(self)
            .into_iter()
            .find(UnresolvedInput::blocks_gates)
    }
}

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
        && ext.test_runner == crate::profile::TestRunner::CargoNextest
        && ext.archive.is_none()
    {
        unresolved.push(UnresolvedInput::ArchiveSource);
    }
    if ext.undeclared_reads {
        unresolved.push(UnresolvedInput::RerunInputs);
    }
    unresolved
}

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
