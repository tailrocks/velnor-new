use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path};
use velnor_actions_contract::config::{CheckRunner, HostContainerProfile};
use velnor_actions_contract::{canonical_json_bytes, digest_b3};
use velnor_actions_mise::checks::{
    CheckCapabilityProof, ContainerObservation, validate_check_capability_proof,
};
pub(crate) mod runtime;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContainerReceipt {
    pub profile_digest: String,
    pub before: CheckCapabilityProof,
    pub after: CheckCapabilityProof,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk: Option<Value>,
    pub runtime: Value,
    pub before_runtime: Option<runtime::RuntimeObservation>,
    pub after_runtime: Option<runtime::RuntimeObservation>,
}
pub(crate) fn container_receipt<S: Serialize, R: Serialize>(
    runner: &CheckRunner,
    before: CheckCapabilityProof,
    after: CheckCapabilityProof,
    sdk: Option<S>,
    runtime: Option<R>,
    before_runtime: Option<runtime::RuntimeObservation>,
    after_runtime: Option<runtime::RuntimeObservation>,
) -> Result<Option<ContainerReceipt>, OrchestratorError> {
    if runner.container.is_none() {
        if before.container.is_some()
            || after.container.is_some()
            || sdk.is_some()
            || runtime.is_some()
            || before_runtime.is_some()
            || after_runtime.is_some()
        {
            return Err(internal("unexpected_container_receipt"));
        }
        return Ok(None);
    }
    let runtime = runtime.ok_or_else(|| internal("container_runtime_receipt_missing"))?;
    let receipt = ContainerReceipt {
        profile_digest: profile_digest(runner)?,
        before,
        after,
        sdk: sdk
            .map(|value| serde_json::to_value(value).map_err(|_| internal("container_sdk_receipt")))
            .transpose()?,
        runtime: serde_json::to_value(runtime)
            .map_err(|_| internal("container_runtime_receipt"))?,
        before_runtime,
        after_runtime,
    };
    validate_container_receipt(runner, Some(&receipt))?;
    Ok(Some(receipt))
}
pub(crate) fn validate_container_receipt(
    runner: &CheckRunner,
    receipt: Option<&ContainerReceipt>,
) -> Result<(), OrchestratorError> {
    match (&runner.container, receipt) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(internal("unexpected_container_receipt")),
        (Some(_), None) => Err(internal("required_container_receipt_missing")),
        (Some(profile), Some(receipt)) => validate_present(runner, profile, receipt),
    }
}
fn validate_present(
    runner: &CheckRunner,
    profile: &HostContainerProfile,
    receipt: &ContainerReceipt,
) -> Result<(), OrchestratorError> {
    if receipt.profile_digest != profile_digest(runner)? {
        return Err(internal("container_profile_digest"));
    }
    for proof in [&receipt.before, &receipt.after] {
        validate_check_capability_proof(runner, proof)
            .map_err(|error| internal(&format!("container_capability:{error}")))?;
    }
    let before = receipt
        .before
        .container
        .as_ref()
        .ok_or_else(|| internal("container_before_missing"))?;
    let after = receipt
        .after
        .container
        .as_ref()
        .ok_or_else(|| internal("container_after_missing"))?;
    validate_continuity(before, after)?;
    validate_owned_paths(profile, before, after)?;
    validate_sdk(profile, receipt.sdk.as_ref(), before, after)?;
    let before_runtime = receipt
        .before_runtime
        .as_ref()
        .ok_or_else(|| internal("container_before_runtime_missing"))?;
    let after_runtime = receipt
        .after_runtime
        .as_ref()
        .ok_or_else(|| internal("container_after_runtime_missing"))?;
    runtime::validate(
        profile,
        &receipt.runtime,
        before,
        after,
        before_runtime,
        after_runtime,
    )
}
fn profile_digest(runner: &CheckRunner) -> Result<String, OrchestratorError> {
    let profile = runner
        .container
        .as_ref()
        .ok_or_else(|| internal("container_profile_missing"))?;
    canonical_json_bytes(profile)
        .map(|bytes| digest_b3(&bytes))
        .map_err(internal_contract)
}
fn validate_continuity(
    before: &ContainerObservation,
    after: &ContainerObservation,
) -> Result<(), OrchestratorError> {
    if before.profile != after.profile
        || before.endpoint != after.endpoint
        || before.docker_program != after.docker_program
        || before.docker_sha256 != after.docker_sha256
        || before.daemon.id.is_empty()
        || after.daemon.id.is_empty()
        || before.daemon.id != after.daemon.id
        || before.daemon.version != after.daemon.version
        || before.daemon.platform != after.daemon.platform
        || before.daemon.architecture != after.daemon.architecture
        || before.daemon.operating_system != after.daemon.operating_system
    {
        return Err(internal("container_execution_identity_changed"));
    }
    match (&before.orbctl, &after.orbctl) {
        (None, None) => Ok(()),
        (Some(left), Some(right))
            if left.program == right.program
                && left.sha256 == right.sha256
                && left.app == right.app =>
        {
            Ok(())
        }
        _ => Err(internal("container_sdk_identity_changed")),
    }
}
fn validate_owned_paths(
    profile: &HostContainerProfile,
    before: &ContainerObservation,
    after: &ContainerObservation,
) -> Result<(), OrchestratorError> {
    for observation in [before, after] {
        if !owned_path(&observation.docker_program, &["bin", "docker"])
            || observation.docker_sha256 != profile.cli().sha256
        {
            return Err(internal("container_owned_cli_path"));
        }
        if let HostContainerProfile::OrbStack { sdk, .. } = profile {
            let orbctl = observation
                .orbctl
                .as_ref()
                .ok_or_else(|| internal("container_owned_sdk_missing"))?;
            if !owned_path(
                &orbctl.program,
                &sdk.cli_relative_path
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>(),
            ) || orbctl.sha256 != sdk.cli_sha256
            {
                return Err(internal("container_owned_sdk_path"));
            }
        }
    }
    Ok(())
}
fn owned_path(path: &Path, suffix: &[&str]) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
        && (suffix.is_empty()
            || (path.ends_with(Path::new(&suffix.join("/")))
                && path.components().count() > suffix.len() + 1))
}
fn validate_sdk(
    profile: &HostContainerProfile,
    sdk: Option<&Value>,
    before: &ContainerObservation,
    after: &ContainerObservation,
) -> Result<(), OrchestratorError> {
    let HostContainerProfile::OrbStack {
        sdk: declaration, ..
    } = profile
    else {
        return sdk
            .is_none()
            .then_some(())
            .ok_or_else(|| internal("container_unexpected_sdk"));
    };
    let Some(sdk) = sdk else {
        return Err(internal("container_sdk_receipt_missing"));
    };
    let object = sdk
        .as_object()
        .ok_or_else(|| internal("container_sdk_shape"))?;
    (object.len() == 15)
        .then_some(())
        .ok_or_else(|| internal("container_sdk_field"))?;
    if object.get("app_bundle").and_then(Value::as_str)
        != Some(declaration.app_bundle_path.as_str())
        || object.get("source_bundle").and_then(Value::as_str)
            != Some(declaration.cli_bundle_path.as_str())
    {
        return Err(internal("container_sdk_declaration"));
    }
    for (name, value) in [
        ("info_plist", &declaration.info_plist_sha256),
        ("main_executable", &declaration.main_executable_sha256),
        ("source_tree", &declaration.source_tree_sha256),
        ("owned_tree", &declaration.owned_tree_sha256),
        ("cli", &declaration.cli_sha256),
    ] {
        for prefix in ["declared_", "observed_"] {
            let key = format!("{prefix}{name}_sha256");
            if object.get(&key).and_then(Value::as_str) != Some(value.as_str()) {
                return Err(internal("container_sdk_declaration"));
            }
        }
    }
    if object.get("cli_sha256").and_then(Value::as_str) != Some(declaration.cli_sha256.as_str()) {
        return Err(internal("container_sdk_declaration"));
    }
    let owned_bundle = object
        .get("owned_bundle")
        .and_then(Value::as_str)
        .ok_or_else(|| internal("container_sdk_owned_bundle"))?;
    let home = before
        .docker_program
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| internal("container_sdk_owned_bundle"))?;
    let source_name = Path::new(&declaration.cli_bundle_path)
        .file_name()
        .ok_or_else(|| internal("container_sdk_owned_bundle"))?;
    if !owned_path(Path::new(owned_bundle), &[])
        || Path::new(owned_bundle) != home.join("orbstack-sdk").join(source_name)
    {
        return Err(internal("container_sdk_owned_bundle"));
    }
    let expected_orbctl = Path::new(owned_bundle).join(&declaration.cli_relative_path);
    for observation in [before, after] {
        let orbctl = observation
            .orbctl
            .as_ref()
            .ok_or_else(|| internal("container_sdk_observation"))?;
        if object.get("orbctl_program").and_then(Value::as_str) != expected_orbctl.to_str()
            || orbctl.program != expected_orbctl
            || orbctl.sha256 != declaration.cli_sha256
        {
            return Err(internal("container_sdk_observation_identity"));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn budget_test_profile() -> HostContainerProfile {
    tests::profile()
}

#[cfg(test)]
pub(crate) fn budget_test_receipt() -> ContainerReceipt {
    tests::receipt()
}
