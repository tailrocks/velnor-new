//! Match reviewed native requirements against the complete preselection universe.
//!
//! Absence cannot reconstruct historical intent. Migration qualification still
//! requires the independent audit, including Rust/Tofu and publication families.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::config::{
    RequiredNativeObligation, RequiredNativeObligations, RequiredNativePhase, WorkloadConfig,
};
use velnor_actions_contract::{
    ProposedTask, Stack, VelnorConfig, canonical_json_bytes, digest_b3, validate_digest,
};

use crate::OrchestratorError;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

pub(crate) const REGISTRY_PATH: &str = ".velnor/required-obligations.toml";

/// Validate required native recipes before any selection or no-work decision.
pub(crate) fn validate(
    root: &Path,
    config: &VelnorConfig,
    universe: &[ProposedTask],
) -> Result<(), OrchestratorError> {
    let RepoRead::Text(text) = read_repo_file(root, REGISTRY_PATH, MAX_REPO_FILE_BYTES)? else {
        return Ok(());
    };
    let registry: RequiredNativeObligations =
        toml::from_str(&text).map_err(|error| invalid(&format!("invalid_registry:{error}")))?;
    validate_registry(&registry, config, universe)
}

/// Canonical semantic recipe: bind every field of the validated declaration.
pub(crate) fn profile_digest(workload: &WorkloadConfig) -> Result<String, OrchestratorError> {
    workload.validate(REGISTRY_PATH)?;
    Ok(digest_b3(&canonical_json_bytes(workload)?))
}

fn validate_registry(
    registry: &RequiredNativeObligations,
    config: &VelnorConfig,
    universe: &[ProposedTask],
) -> Result<(), OrchestratorError> {
    registry.validate(REGISTRY_PATH)?;
    for entry in &registry.obligations {
        validate_entry(entry, config, universe)?;
    }
    Ok(())
}

fn validate_entry(
    entry: &RequiredNativeObligation,
    config: &VelnorConfig,
    universe: &[ProposedTask],
) -> Result<(), OrchestratorError> {
    validate_digest(&entry.profile_digest)?;
    let workload = config
        .stacks
        .workloads
        .iter()
        .find(|workload| workload.name == entry.component)
        .ok_or_else(|| invalid(&format!("missing_declaration:{}", entry.component)))?;
    if workload.kind != entry.operation || workload.root != entry.root {
        return Err(invalid(&format!(
            "declaration_mismatch:{}",
            entry.component
        )));
    }
    if profile_digest(workload)? != entry.profile_digest {
        return Err(invalid(&format!("profile_mismatch:{}", entry.component)));
    }
    let mut declared = BTreeSet::new();
    for phase in &entry.phases {
        if !declared.insert(phase.id()) {
            return Err(invalid(&format!("duplicate_phase:{}", entry.component)));
        }
    }
    if declared.is_empty() {
        return Err(invalid("empty_phase_inventory"));
    }
    validate_emission(entry, workload, &declared, universe)
}

fn validate_emission(
    entry: &RequiredNativeObligation,
    workload: &WorkloadConfig,
    expected: &BTreeSet<&str>,
    universe: &[ProposedTask],
) -> Result<(), OrchestratorError> {
    let kind = crate::workloads::kind_id(workload.kind);
    let owned: Vec<_> = universe
        .iter()
        .filter(|task| {
            task.stack_id == Stack::Workload.id()
                && task.identity.unit_id == format!("workload:{}", entry.component)
        })
        .collect();
    if owned.iter().any(|task| {
        task.configuration != kind
            || task.identity.unit_key != entry.component
            || task.component_id != entry.component
            || task.identity.unit_path != entry.root.as_str()
            || task.task_id
                != format!(
                    "stack/workload/{}/{}/{}",
                    entry.component, task.task_kind, kind
                )
    }) {
        return Err(invalid(&format!(
            "emission_identity_mismatch:{}",
            entry.component
        )));
    }
    let actual: BTreeSet<_> = owned
        .iter()
        .map(|task| {
            RequiredNativePhase::from_id(&task.task_kind)
                .map(RequiredNativePhase::id)
                .ok_or_else(|| invalid("unknown_compiled_phase"))
        })
        .collect::<Result<_, _>>()?;
    if actual.len() != owned.len() || &actual != expected {
        return Err(invalid(&format!(
            "phase_inventory_mismatch:{}",
            entry.component
        )));
    }
    Ok(())
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::config(REGISTRY_PATH, "obligations", problem)
}

#[cfg(test)]
#[path = "required_workloads_tests.rs"]
mod tests;
