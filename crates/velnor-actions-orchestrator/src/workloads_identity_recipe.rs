//! Typed desktop generation identity bound to immutable task proposals.

use velnor_actions_contract::config::{NativeDesktopProfile, Utf8RepoRelDir, WorkloadConfig};
use velnor_actions_contract::{ProposedTask, Stack, canonical_json_bytes};

use crate::OrchestratorError;

pub(crate) const NATIVE_DESKTOP_PROFILE_KEY: &str = "VELNOR_NATIVE_DESKTOP_PROFILE";
const MAX_DESCRIPTOR_BYTES: usize = 4 * 1024 * 1024;

/// Decode one closed profile shared by every phase of the same workload.
pub(crate) fn desktop_profile(
    tasks: &[&ProposedTask],
) -> Result<Option<NativeDesktopProfile>, OrchestratorError> {
    let Some(first) = tasks.first() else {
        return Ok(None);
    };
    if !tasks.iter().any(|task| {
        native_kind(&task.configuration)
            || task
                .identity
                .environment
                .contains_key(NATIVE_DESKTOP_PROFILE_KEY)
    }) {
        return Ok(None);
    }
    let profile = decode(first)?;
    for task in tasks.iter().skip(1) {
        if !same_owner(first, task) {
            return Err(invalid("mixed_group_identity"));
        }
        if decode(task)? != profile {
            return Err(invalid("contradictory_group_descriptors"));
        }
    }
    Ok(Some(profile))
}

/// Canonical validated desktop descriptor for the toolchain preimage.
pub(crate) fn profile_identity(task: &ProposedTask) -> Result<Option<String>, OrchestratorError> {
    desktop_profile(&[task])?
        .map(|profile| {
            String::from_utf8(canonical_json_bytes(&profile)?)
                .map_err(|_| invalid("non_utf8_descriptor"))
        })
        .transpose()
}

fn decode(task: &ProposedTask) -> Result<NativeDesktopProfile, OrchestratorError> {
    if task.stack_id != Stack::Workload.id() || !native_kind(&task.configuration) {
        return Err(invalid("wrong_workload_kind"));
    }
    if task.identity.project_root != "." {
        return Err(invalid("desktop_checkout_root_required"));
    }
    if task.component_id != task.identity.unit_key
        || task.identity.unit_id != format!("workload:{}", task.identity.unit_key)
    {
        return Err(invalid("contradictory_workload_identity"));
    }
    let raw = task
        .identity
        .environment
        .get(NATIVE_DESKTOP_PROFILE_KEY)
        .ok_or_else(|| invalid("missing_required_profile"))?;
    if raw.len() > MAX_DESCRIPTOR_BYTES {
        return Err(invalid("descriptor_size"));
    }
    let profile: NativeDesktopProfile =
        serde_json::from_str(raw).map_err(|_| invalid("malformed_descriptor"))?;
    let workload = WorkloadConfig {
        name: task.identity.unit_key.clone(),
        kind: serde_json::from_value(serde_json::Value::String(task.configuration.clone()))
            .map_err(|_| invalid("wrong_workload_kind"))?,
        root: Utf8RepoRelDir::parse(&task.identity.unit_path)
            .map_err(|_| invalid("invalid_workload_root"))?,
        inputs: Vec::new(),
        paths: Vec::new(),
        scripts: None,
        gradle: None,
        native_desktop: Some(profile.clone()),
        package_update: None,
    };
    workload.validate("task.identity.environment")?;
    Ok(profile)
}

fn same_owner(first: &ProposedTask, task: &ProposedTask) -> bool {
    task.stack_id == first.stack_id
        && task.configuration == first.configuration
        && task.component_id == first.component_id
        && task.identity.unit_id == first.identity.unit_id
        && task.identity.unit_key == first.identity.unit_key
        && task.identity.unit_path == first.identity.unit_path
        && task.identity.project_root == first.identity.project_root
}

fn native_kind(kind: &str) -> bool {
    matches!(kind, "native_xcode_project_ci" | "native_swift_package_ci")
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("workload_generation_metadata:{NATIVE_DESKTOP_PROFILE_KEY}:{problem}"),
    }
}

#[cfg(test)]
#[path = "workloads_identity_recipe_tests.rs"]
mod tests;
