//! Generation-only semantic requests; compiled native owners supply authority.

use std::ffi::OsString;
use velnor_actions_contract::config::{WorkloadConfig, WorkloadKind, is_valid_workload_name};
use velnor_actions_contract::{
    NativeValidationDescriptor, ProposedTask, Stack, canonical_json_bytes, parse_strict_json,
};
use velnor_actions_native::homebrew::{audit, package_update, preparation};

use crate::OrchestratorError;

pub(crate) const NATIVE_VALIDATION_DESCRIPTOR_KEY: &str = "VELNOR_NATIVE_VALIDATION_DESCRIPTOR";
const MAX_DESCRIPTOR_BYTES: usize = 128 * 1024;

/// Capture validated semantic data before constructing a proposal's identity.
pub(crate) fn from_workload(
    workload: &WorkloadConfig,
    phase: &str,
    payload: &[OsString],
) -> Result<Option<NativeValidationDescriptor>, OrchestratorError> {
    workload.validate(".velnor/config.toml")?;
    let descriptor = match workload.kind {
        WorkloadKind::PackageUpdateFixture => {
            if phase != package_update::PHASE || workload.root.as_str() != "." {
                return Err(invalid("package_phase_or_root"));
            }
            let profile = workload
                .package_update
                .as_ref()
                .ok_or_else(|| invalid("missing_package_profile"))?;
            NativeValidationDescriptor::PackageUpdateFixture {
                profile: profile.clone(),
            }
        }
        WorkloadKind::HomebrewAudit if phase == "homebrew-tap-local" => {
            if workload.root.as_str() != "." {
                return Err(invalid("homebrew_root"));
            }
            homebrew_descriptor(payload)?
        }
        _ => return Ok(None),
    };
    descriptor.validate()?;
    if canonical_json_bytes(&descriptor)?.len() > MAX_DESCRIPTOR_BYTES {
        return Err(invalid("descriptor_size"));
    }
    validate_payload(&descriptor, payload)?;
    Ok(Some(descriptor))
}

/// Read only an exact descriptor bound to its closed logical proposal.
pub(crate) fn from_proposal(
    task: &ProposedTask,
) -> Result<Option<NativeValidationDescriptor>, OrchestratorError> {
    if task.stack_id != Stack::Workload.id() {
        return if task
            .identity
            .environment
            .contains_key(NATIVE_VALIDATION_DESCRIPTOR_KEY)
        {
            Err(invalid("unexpected_descriptor"))
        } else {
            Ok(None)
        };
    }
    let required = match (task.configuration.as_str(), task.task_kind.as_str()) {
        ("package_update_fixture", package_update::PHASE) => true,
        ("package_update_fixture", _) => return Err(invalid("package_phase")),
        ("homebrew_audit", "homebrew-tap-local") => true,
        _ => false,
    };
    let Some(raw) = task
        .identity
        .environment
        .get(NATIVE_VALIDATION_DESCRIPTOR_KEY)
    else {
        return if required {
            Err(invalid("missing_required_descriptor"))
        } else {
            Ok(None)
        };
    };
    if !required {
        return Err(invalid("unexpected_descriptor"));
    }
    validate_identity(task)?;
    if raw.len() > MAX_DESCRIPTOR_BYTES {
        return Err(invalid("descriptor_size"));
    }
    let descriptor: NativeValidationDescriptor = serde_json::from_value(parse_strict_json(raw)?)
        .map_err(|_| invalid("malformed_descriptor"))?;
    descriptor.validate()?;
    if canonical_json_bytes(&descriptor)? != raw.as_bytes() {
        return Err(invalid("noncanonical_descriptor"));
    }
    let kind_matches = matches!(
        (&descriptor, task.configuration.as_str()),
        (
            NativeValidationDescriptor::PackageUpdateFixture { .. },
            "package_update_fixture"
        ) | (
            NativeValidationDescriptor::HomebrewPreparation { .. },
            "homebrew_audit"
        )
    );
    if !kind_matches {
        return Err(invalid("descriptor_kind"));
    }
    validate_payload(&descriptor, &task.payload)?;
    Ok(Some(descriptor))
}

fn validate_identity(task: &ProposedTask) -> Result<(), OrchestratorError> {
    let name = &task.component_id;
    if task.stack_id != Stack::Workload.id()
        || !is_valid_workload_name(name)
        || task.task_id
            != format!(
                "stack/workload/{name}/{}/{}",
                task.task_kind, task.configuration
            )
        || task.identity.unit_id != format!("workload:{name}")
        || task.identity.unit_key != *name
        || task.identity.unit_path != "."
        || task.identity.project_root != "."
        || task.identity.compile_driver != "native"
        || task.identity.test_runner != "native"
        || task.identity.target != "host"
        || task.runner_profile != "native"
    {
        return Err(invalid("proposal_identity"));
    }
    Ok(())
}

fn validate_payload(
    descriptor: &NativeValidationDescriptor,
    payload: &[OsString],
) -> Result<(), OrchestratorError> {
    let expected = match descriptor {
        NativeValidationDescriptor::PackageUpdateFixture { profile } => {
            package_update::arguments(profile)?
        }
        NativeValidationDescriptor::HomebrewPreparation {
            repository,
            has_casks,
        } => {
            let tap = audit::TapIdentity::from_repository(repository)?;
            let mut arguments = vec!["homebrew-preparation".to_owned()];
            arguments.extend(preparation::preparation_arguments(
                &source_identity()?,
                &tap,
                *has_casks,
            ));
            arguments
        }
    };
    if expected.iter().map(OsString::from).collect::<Vec<_>>() != payload {
        return Err(invalid("logical_payload_mismatch"));
    }
    Ok(())
}

fn homebrew_descriptor(
    payload: &[OsString],
) -> Result<NativeValidationDescriptor, OrchestratorError> {
    let arguments = payload
        .iter()
        .map(|argument| argument.to_str().ok_or_else(|| invalid("payload_not_utf8")))
        .collect::<Result<Vec<_>, _>>()?;
    let ["homebrew-preparation", owner, name, _, _, capability] = arguments.as_slice() else {
        return Err(invalid("homebrew_payload"));
    };
    let repository = format!("{owner}/homebrew-{name}");
    let has_casks = match *capability {
        "casks" => true,
        "formula-only" => false,
        _ => return Err(invalid("homebrew_capability")),
    };
    Ok(NativeValidationDescriptor::HomebrewPreparation {
        repository,
        has_casks,
    })
}

fn source_identity() -> Result<audit::BrewSourceIdentity, OrchestratorError> {
    use velnor_actions_mise::catalog::homebrew;
    Ok(audit::BrewSourceIdentity::reviewed(
        homebrew::VERSION,
        homebrew::SOURCE_SHA,
        homebrew::PORTABLE_RUBY_VERSION,
        homebrew::PORTABLE_RUBY_X86_64_LINUX_SHA256,
        homebrew::PORTABLE_RUBY_ARM64_LINUX_SHA256,
    )?)
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("native_validation_descriptor:{problem}"),
    }
}

#[cfg(test)]
#[path = "workloads_native_descriptor_tests.rs"]
mod tests;
