//! Generator-owned package updater cases with typed source and artifact mappings.

use crate::{OwnedSupportFile, SupportBundle};
use velnor_actions_contract::config::PackageUpdateFixture;
use velnor_actions_contract::{ContractError, FileIndex, SourceBoundOperation};

#[path = "package_update_script.rs"]
mod script;

/// One generator-owned source-bound operation, independent of repository names.
pub const OPERATION: SourceBoundOperation = SourceBoundOperation::PackageUpdateFixture;
/// Fixed obligation phase, independent of output/artifact mappings.
pub const PHASE: &str = "package-update-fixtures";

/// Semantic tools; Mise alone lowers these requirements to exact selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageUpdateTool {
    /// Syntax and generated artifact mapping checks through standard Ripper.
    Ruby,
    /// Fixed package manifest and fixture data operations.
    Jq,
}

/// The compiled harness copies the complete indexed source checkout.
#[must_use]
pub fn inputs(index: &FileIndex) -> Vec<String> {
    index.files().to_vec()
}

/// Validate source evidence without reading or executing repository programs.
/// # Errors
/// Rejects malformed profiles, nested roots and absent indexed source inputs.
pub fn validate_evidence(
    profile: &PackageUpdateFixture,
    root: &str,
    index: &FileIndex,
) -> Result<(), ContractError> {
    profile.validate(".velnor/config.toml", "stacks.workloads.package_update")?;
    if root != "." {
        return Err(ContractError::identity(
            "package_update_fixture",
            "requires_repository_root",
        ));
    }
    for path in [&profile.updater, &profile.formula, &profile.preview_formula]
        .into_iter()
        .chain(profile.cask.iter())
    {
        if !index.contains(path) {
            return Err(ContractError::identity(
                "package_update_fixture",
                format!("source_evidence_missing:{path}"),
            ));
        }
    }
    Ok(())
}

/// Native tools required by this fixed operation, never installation authority.
#[must_use]
pub const fn tools() -> [PackageUpdateTool; 2] {
    [PackageUpdateTool::Ruby, PackageUpdateTool::Jq]
}

/// Canonical typed data is the helper's sole argument; no task/command selectors.
/// # Errors
/// Rejects malformed profiles or serialization failures.
pub fn arguments(profile: &PackageUpdateFixture) -> Result<Vec<String>, ContractError> {
    profile.validate(".velnor/config.toml", "stacks.workloads.package_update")?;
    let json = serde_json::to_string(profile).map_err(|_| {
        ContractError::identity("package_update_fixture", "profile_serialization_failed")
    })?;
    Ok(vec![json])
}

/// Complete immutable helper closure. Native owner never lowers Mise execution.
/// # Errors
/// Rejects invalid generated source/version markers.
pub fn support(version: &str) -> Result<SupportBundle, ContractError> {
    let file = OwnedSupportFile::compiled(OPERATION.path(), &script::compiled(), version)?;
    SupportBundle::compiled(vec![file])
}

/// Fixed phase rank in a native validation job.
#[must_use]
pub fn rank(phase: &str) -> Option<u32> {
    (phase == PHASE).then_some(3)
}

/// Human label independent of repository-specific artifact names.
#[must_use]
pub fn step_name(phase: &str) -> Option<&'static str> {
    (phase == PHASE).then_some("Test package updater fixtures")
}

#[cfg(test)]
#[path = "package_update_tests.rs"]
mod tests;
