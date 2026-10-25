use super::tree::{caller_repository, pull_request_number_from_env};
use super::{
    BTreeMap, CheckDeadline, Duration, Instant, NativeHomebrewCheck, NativeHomebrewOperation,
    PackageReleaseChannel, Path, SeedTransitionAdmission, Stack,
};
use super::{inventory::protected_stable_is_activated, seed::derive_seed_transition};
use crate::{OrchestratorError, internal::internal};
use stable::{
    protected_stable_head_is_activated, stable_install_required, stable_or_preview_install_check,
};

#[path = "native_homebrew_check_required_stable.rs"]
mod stable;

pub(crate) fn validate_required_plan(
    root: &Path,
    plan: &velnor_actions_contract::Plan,
) -> Result<(), crate::OrchestratorError> {
    use crate::internal::internal;
    let repository = caller_repository(root)?;
    let deadline =
        velnor_actions_mise::CheckDeadline::from_start(Instant::now(), Duration::from_mins(5))
            .map_err(|error| internal(&error.to_string()))?;
    let pull_request = pull_request_number_from_env(plan.event)?;
    if let Some(seed) = derive_seed_transition(
        root,
        &repository,
        plan.event,
        pull_request,
        &plan.head,
        deadline,
        false,
    )? {
        return validate_seed_required_plan(plan, &seed);
    }
    route_required_admission(
        plan,
        || {
            crate::homebrew::authority::protected_locator_absent_on_default_branch(
                &repository,
                deadline,
            )
        },
        || validate_full_required_plan(root, &repository, plan, pull_request, deadline),
    )
}

fn route_required_admission(
    plan: &velnor_actions_contract::Plan,
    prove_locator_absent: impl FnOnce() -> Result<bool, OrchestratorError>,
    full_admission: impl FnOnce() -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    if !has_native_matrix_entry(&plan.matrix.include) && prove_locator_absent()? {
        return Ok(());
    }
    full_admission()
}

fn validate_full_required_plan(
    root: &Path,
    repository: &str,
    plan: &velnor_actions_contract::Plan,
    pull_request: Option<u64>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let authority = crate::homebrew::authority::probe_protected_homebrew_authority(
        root,
        repository,
        plan.event,
        pull_request,
        &plan.head,
        deadline,
    )?;
    let actual = actual_native_checks(&plan.matrix.include);
    let Some(authority) = authority else {
        require_native_authority(&plan.matrix.include)?;
        return Ok(());
    };
    validate_source_required_plan(root, &authority, &actual, deadline)
}

fn has_native_matrix_entry(entries: &[velnor_actions_contract::MatrixEntry]) -> bool {
    entries
        .iter()
        .any(|entry| entry.stack_id == Stack::Native.id())
}

fn require_native_authority(
    entries: &[velnor_actions_contract::MatrixEntry],
) -> Result<(), OrchestratorError> {
    if has_native_matrix_entry(entries) {
        Err(internal("native_checks_without_protected_authority"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_homebrew_check_required_applicability_tests.rs"]
mod applicability_tests;

fn validate_seed_required_plan(
    plan: &velnor_actions_contract::Plan,
    seed: &SeedTransitionAdmission,
) -> Result<(), crate::OrchestratorError> {
    let transition = seed.transition.transition();
    let transition_digest = transition
        .digest()
        .map_err(crate::internal::internal_contract)?;
    let entries = plan
        .matrix
        .include
        .iter()
        .map(|entry| (entry.stack_id.as_str(), &entry.adapter_metadata));
    for required in &transition.required_operations {
        if !seed_required_operation_present(
            entries.clone(),
            *required,
            transition,
            &transition_digest,
            &seed.sdk,
        ) {
            return Err(crate::internal::internal(
                "native_required_seed_operation_missing",
            ));
        }
    }
    Ok(())
}

type ActualNativeCheck = (String, Option<String>, String, String);

fn actual_native_checks(
    entries: &[velnor_actions_contract::MatrixEntry],
) -> BTreeMap<String, ActualNativeCheck> {
    entries
        .iter()
        .filter(|entry| entry.stack_id == Stack::Native.id())
        .filter_map(|entry| {
            let id = entry.adapter_metadata.get("check_id")?.as_str()?;
            let operation = entry.adapter_metadata.get("operation")?.as_str()?;
            let identity = entry.adapter_metadata.get("identity")?;
            Some((
                id.to_owned(),
                (
                    operation.to_owned(),
                    identity.get("formula_channel")?.as_str().map(str::to_owned),
                    identity.get("platform")?.as_str()?.to_owned(),
                    identity.get("runner")?.as_str()?.to_owned(),
                ),
            ))
        })
        .collect()
}

fn validate_source_required_plan(
    root: &Path,
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    actual: &BTreeMap<String, ActualNativeCheck>,
    deadline: CheckDeadline,
) -> Result<(), crate::OrchestratorError> {
    let tap = authority.tap();
    if !tap.enabled {
        return Err(crate::internal::internal("native_protected_tap_disabled"));
    }
    let cutover =
        crate::homebrew::structural_cutover::verify_seed_output_base(root, authority, deadline)?;
    if let Some(witness) = cutover.as_ref() {
        crate::homebrew::structural_cutover::validate_structural_candidate(
            root, authority, witness, deadline,
        )?;
    }
    let candidate_config = crate::config::load_config(root)?;
    let checks = required_native_homebrew_checks(authority, cutover.as_ref(), deadline)?;
    for check in checks {
        validate_required_declaration(&candidate_config, &check)?;
        validate_required_matrix_entry(actual, &check)?;
    }
    Ok(())
}

fn validate_required_declaration(
    config: &velnor_actions_contract::VelnorConfig,
    check: &NativeHomebrewCheck,
) -> Result<(), crate::OrchestratorError> {
    if config
        .native_checks
        .iter()
        .any(|configured| configured == check)
    {
        Ok(())
    } else {
        Err(crate::internal::internal(
            "native_required_declaration_mismatch",
        ))
    }
}

fn validate_required_matrix_entry(
    actual: &BTreeMap<String, ActualNativeCheck>,
    check: &NativeHomebrewCheck,
) -> Result<(), crate::OrchestratorError> {
    let channel = check.formula_channel.map(|channel| match channel {
        PackageReleaseChannel::Preview => "preview".to_owned(),
        PackageReleaseChannel::Stable => "stable".to_owned(),
    });
    let platform = match check.runner.platform {
        velnor_actions_contract::config::CheckPlatform::MacosArm64 => "macos_arm64",
        velnor_actions_contract::config::CheckPlatform::LinuxX64 => "linux_x64",
        velnor_actions_contract::config::CheckPlatform::MacosX64 => "macos_x64",
    };
    let matches = actual.get(&check.id).is_some_and(
        |(operation, actual_channel, actual_platform, runner)| {
            operation == check.operation.as_str()
                && actual_channel == &channel
                && actual_platform == platform
                && runner == &check.runner.label
        },
    );
    if matches {
        Ok(())
    } else {
        Err(crate::internal::internal(
            "native_required_operation_missing",
        ))
    }
}

/// Shared source-authorized declarations used by Required and cutover replay.
pub(crate) fn required_native_homebrew_checks(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    cutover: Option<&crate::homebrew::structural_cutover::PrivateVerifiedSeedOutputBase>,
    deadline: CheckDeadline,
) -> Result<Vec<velnor_actions_contract::config::NativeHomebrewCheck>, OrchestratorError> {
    use velnor_actions_contract::config::{
        CheckExecutor, CheckPlatform, CheckRunner, NativeHomebrewCheck,
    };
    let tap = authority.tap();
    if !tap.enabled {
        return Err(internal("native_protected_tap_disabled"));
    }
    let release = authority
        .source_config()
        .stacks
        .rust
        .as_ref()
        .map(|stack| &stack.binary_release);
    let mut checks = vec![NativeHomebrewCheck {
        id: "tap_tree".to_owned(),
        operation: NativeHomebrewOperation::TapTree,
        formula_channel: None,
        runner: CheckRunner {
            label: "ubuntu-26.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        timeout_minutes: 5,
    }];
    for (channel, _) in tap.destinations.iter() {
        let channel_name = match channel {
            PackageReleaseChannel::Preview => "preview",
            PackageReleaseChannel::Stable => "stable",
        };
        checks.push(NativeHomebrewCheck {
            id: format!("formula_contract_{channel_name}"),
            operation: NativeHomebrewOperation::FormulaContract,
            formula_channel: Some(channel),
            runner: CheckRunner {
                label: "ubuntu-26.04".to_owned(),
                platform: CheckPlatform::LinuxX64,
                executor: CheckExecutor::Hosted,
                container: None,
            },
            timeout_minutes: 5,
        });
        let activated = match channel {
            PackageReleaseChannel::Preview => release.is_some_and(|config| config.preview.enabled),
            PackageReleaseChannel::Stable => {
                let base_active = protected_stable_is_activated(authority, cutover, deadline)?;
                let head_active = if authority.tap_head_sha() == authority.tap_base_sha() {
                    base_active
                } else {
                    protected_stable_head_is_activated(authority, deadline)?
                };
                stable_install_required(base_active, head_active)
            }
        };
        if activated {
            checks.push(stable_or_preview_install_check(channel));
        }
    }
    Ok(checks)
}

pub(super) fn seed_required_operation_present<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a serde_json::Value)>,
    required_operation: NativeHomebrewOperation,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    transition_digest: &str,
    sdk: &crate::homebrew::sdk_binding::PublishedSdkBindingIdentity,
) -> bool {
    entries.into_iter().any(|(stack_id, metadata)| {
        seed_check_metadata_matches(
            stack_id,
            metadata,
            required_operation,
            transition,
            transition_digest,
            sdk,
        )
    })
}

pub(super) fn seed_check_metadata_matches(
    stack_id: &str,
    metadata: &serde_json::Value,
    required_operation: NativeHomebrewOperation,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    transition_digest: &str,
    sdk: &crate::homebrew::sdk_binding::PublishedSdkBindingIdentity,
) -> bool {
    if stack_id != Stack::Native.id()
        || metadata
            .get("operation")
            .and_then(serde_json::Value::as_str)
            != Some(required_operation.as_str())
    {
        return false;
    }
    let Some(identity) = metadata.get("identity") else {
        return false;
    };
    identity
        .get("tap_base_sha")
        .and_then(serde_json::Value::as_str)
        == Some(transition.base_sha.as_str())
        && identity.get("authority").is_some_and(|authority| {
            authority.get("kind").and_then(serde_json::Value::as_str) == Some("seed")
                && authority
                    .get("transition_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(transition.transition_id.as_str())
                && authority
                    .get("transition_digest")
                    .and_then(serde_json::Value::as_str)
                    == Some(transition_digest)
                && authority
                    .get("base_tree")
                    .and_then(serde_json::Value::as_str)
                    == Some(transition.base_tree.as_str())
                && authority
                    .get("sdk_source_sha")
                    .and_then(serde_json::Value::as_str)
                    == Some(sdk.source_sha.as_str())
                && authority
                    .get("sdk_version")
                    .and_then(serde_json::Value::as_str)
                    == Some(sdk.version.as_str())
                && authority
                    .get("sdk_manifest_sha256")
                    .and_then(serde_json::Value::as_str)
                    == Some(sdk.manifest_sha256.as_str())
                && authority
                    .get("sdk_seed_runner_binary_sha256")
                    .and_then(serde_json::Value::as_str)
                    == Some(sdk.seed_runner_binary_sha256.as_str())
        })
}
