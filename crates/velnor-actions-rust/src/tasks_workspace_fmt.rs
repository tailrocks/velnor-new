use velnor_actions_contract::{ContractError, manifest_key_for_cargo_manifest, task_id_for_stack};

use crate::profile::RustExecutionProfile;

use super::{TaskGroup, TaskKind};

/// Derive the single plan-level formatting group for a workspace root.
///
/// # Errors
///
/// Returns [`ContractError`] when the workspace manifest or task id is invalid.
pub fn derive_workspace_fmt(
    workspace_manifest: &str,
    profile: &RustExecutionProfile,
    configuration: &str,
    target: &str,
) -> Result<TaskGroup, ContractError> {
    derive_workspace_fmt_with_features(workspace_manifest, profile, configuration, target, false)
}

/// Make the single plan-level formatting group with its configuration mode.
///
/// # Errors
///
/// Returns [`ContractError`] when the workspace manifest or task id is invalid.
pub fn derive_workspace_fmt_with_features(
    workspace_manifest: &str,
    profile: &RustExecutionProfile,
    configuration: &str,
    target: &str,
    all_features: bool,
) -> Result<TaskGroup, ContractError> {
    let key = manifest_key_for_cargo_manifest(workspace_manifest)?;
    let task_id = task_id_for_stack(
        crate::STACK_ID,
        &key,
        TaskKind::Fmt.as_str(),
        configuration,
        None,
    )?;
    Ok(TaskGroup {
        task_id,
        package_id: String::new(),
        package_name: String::new(),
        manifest_key: key,
        kind: TaskKind::Fmt,
        configuration: configuration.to_owned(),
        features: Vec::new(),
        all_features,
        target: target.to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: profile.compile_driver,
        test_runner: profile.test_runner,
        nextest_profile: profile.nextest_profile,
        run_ignored: None,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    })
}

/// Derive the workspace formatting group only with explicit root config.
///
/// Returns `None` without explicit workspace formatting configuration:
/// per-package groups cover explicit packages and the plan-job Format
/// step covers the rest, so a workspace matrix obligation would double-run.
///
/// # Errors
///
/// Returns [`ContractError`] when the workspace manifest or task id is invalid.
pub fn derive_workspace_fmt_if_explicit(
    workspace_manifest: &str,
    profile: &RustExecutionProfile,
    configuration: &str,
    target: &str,
    explicit_fmt: bool,
) -> Result<Option<TaskGroup>, ContractError> {
    derive_workspace_fmt_if_explicit_with_features(
        workspace_manifest,
        profile,
        configuration,
        target,
        explicit_fmt,
        false,
    )
}

/// Make the workspace formatting group with its all-features identity mode.
///
/// # Errors
///
/// Returns [`ContractError`] when the workspace manifest or task id is invalid.
pub fn derive_workspace_fmt_if_explicit_with_features(
    workspace_manifest: &str,
    profile: &RustExecutionProfile,
    configuration: &str,
    target: &str,
    explicit_fmt: bool,
    all_features: bool,
) -> Result<Option<TaskGroup>, ContractError> {
    if !explicit_fmt {
        return Ok(None);
    }
    derive_workspace_fmt_with_features(
        workspace_manifest,
        profile,
        configuration,
        target,
        all_features,
    )
    .map(Some)
}
