//! Runtime identity evidence and exact cache key binding.

use serde::{Deserialize, Serialize};

use super::receipt::{QualificationCacheBackendEntry, QualificationCacheBackendObservation};
use crate::canonical::{canonical_json_bytes, digest_b3, validate_digest};
use crate::errors::ContractError;

/// Runtime fact a cache producer must collect before binding a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationRuntimeIdentityField {
    /// Actual hosted runner OS.
    RunnerOs,
    /// Actual hosted runner architecture.
    RunnerArch,
    /// Actual provisioned image family.
    ImageOs,
    /// Actual provisioned image version.
    ImageVersion,
    /// Toolchain identity observed after pinned tool setup.
    ToolchainId,
    /// Cache-format identity reported by its producer.
    CacheFormatId,
    /// ABI identity reported by the actual runner and toolchain.
    AbiId,
    /// Selected compile-driver/configuration identity.
    DriverId,
}

/// Runtime platform dimensions reported by the active runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimePlatform {
    /// Operating system name.
    pub os: String,
    /// CPU architecture.
    pub arch: String,
    /// Exact literal `runs-on` label.
    pub runs_on: String,
    /// Provisioned image family.
    pub image_os: String,
    /// Provisioned image version.
    pub image_version: String,
    /// Execution target (`host` or a triple).
    pub target: String,
}

/// Runtime facts needed to bind a logical slot to one exact cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeIdentity {
    /// Runtime OS, architecture, label, image and target evidence.
    pub platform: QualificationRuntimePlatform,
    /// Toolchain identity verified after pinned tool setup.
    pub toolchain_id: String,
    /// Cache-format identity reported by its producer.
    pub cache_format_id: String,
    /// ABI identity reported by the actual runner and toolchain.
    pub abi_id: String,
    /// Adapter metadata digest binding the selected driver and configuration.
    pub driver_id: String,
}

/// Plan-known cache identity values plus runtime facts still required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeIdentityRequirements {
    /// Exact validated `runs-on` label from the plan.
    pub runner_label: String,
    /// Execution target associated with that runner label.
    pub target: String,
    /// Toolchain digest recorded by the matrix entry.
    pub toolchain_id: String,
    /// Cache-format digest recorded by the matrix entry.
    pub cache_format_id: String,
    /// ABI identity must be present and match the admitted predecessor.
    pub require_abi: bool,
    /// Selected driver/configuration digest from the matrix entry.
    pub driver_id: String,
    /// Fields that must be present in runtime evidence before cache access.
    pub required_fields: Vec<QualificationRuntimeIdentityField>,
}

/// Required cache object for one restore request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheRestoreExpectation {
    /// The exact primary key submitted to the action.
    pub requested_key: String,
    /// Expected immutable cache object, or None when a miss is required.
    pub expected_cache: Option<QualificationCacheBackendEntry>,
}

/// Exact bound keys emitted only after runtime identity verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundQualificationCacheKeys {
    /// Restore request and required post-action admission evidence.
    pub restore: Option<QualificationCacheRestoreExpectation>,
    /// Key to save after comparing actual layer-state digest.
    pub save_key: Option<String>,
    /// Whether saving is allowed only when state changed.
    pub save_if_state_changes: bool,
    /// Admitted predecessor layer-state digest, if present.
    pub expected_prior_state_digest: Option<String>,
    /// Digest of the observed runtime identity included in each key.
    pub runtime_identity_digest: Option<String>,
}

impl BoundQualificationCacheKeys {
    /// Admit a staged restore only after its actual match and cache object agree.
    ///
    /// Cache actions can restore and extract a prefix match even when their
    /// restore-key list is empty. Restore into an isolated staging directory,
    /// call this method, and discard staged bytes on error before import/use.
    /// # Errors
    pub fn validate_restore_observation(
        &self,
        matched_key: Option<&str>,
        observed: &QualificationCacheBackendObservation,
    ) -> Result<(), ContractError> {
        let Some(expectation) = &self.restore else {
            return if matched_key.is_none()
                && *observed == QualificationCacheBackendObservation::NotQueried
            {
                Ok(())
            } else {
                Err(ContractError::identity(
                    "qualification.cache_restore",
                    "unexpected_restore_observation",
                ))
            };
        };
        match (&expectation.expected_cache, observed) {
            (None, QualificationCacheBackendObservation::Absent) if matched_key.is_none() => Ok(()),
            (Some(expected), QualificationCacheBackendObservation::Found(actual))
                if matched_key == Some(expectation.requested_key.as_str())
                    && actual == expected
                    && actual.key == expectation.requested_key =>
            {
                Ok(())
            }
            _ => Err(ContractError::identity(
                "qualification.cache_restore",
                "actual_match_not_admitted",
            )),
        }
    }

    /// Return the save key only when the observed state meets the phase policy.
    /// # Errors
    pub fn save_key_for_state(&self, state_digest: &str) -> Result<Option<&str>, ContractError> {
        validate_digest(state_digest)?;
        let Some(key) = self.save_key.as_deref() else {
            return Ok(None);
        };
        if self.save_if_state_changes
            && self.expected_prior_state_digest.as_deref() == Some(state_digest)
        {
            return Ok(None);
        }
        Ok(Some(key))
    }
}

/// Check runtime observations against plan identity and derive their digest.
pub(crate) fn validate_runtime_identity(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
) -> Result<String, ContractError> {
    use crate::cachekey::{PlatformInputs, platform_id, validate_semantic_text};

    let platform = &evidence.platform;
    if platform.runs_on != requirements.runner_label || platform.target != requirements.target {
        return Err(ContractError::identity(
            "qualification.runtime_identity",
            "runner_binding_mismatch",
        ));
    }
    for (field, value) in [
        ("runner_os", platform.os.as_str()),
        ("runner_arch", platform.arch.as_str()),
        ("image_os", platform.image_os.as_str()),
        ("image_version", platform.image_version.as_str()),
    ] {
        validate_semantic_text(field, value)?;
        if value == crate::UNOBSERVED_IMAGE_VALUE {
            return Err(ContractError::identity(
                "qualification.runtime_identity",
                format!("unobserved:{field}"),
            ));
        }
    }
    if evidence.toolchain_id != requirements.toolchain_id
        || evidence.cache_format_id != requirements.cache_format_id
        || evidence.driver_id != requirements.driver_id
        || !requirements.require_abi
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity",
            "toolchain_format_or_driver_mismatch",
        ));
    }
    validate_digest(&evidence.toolchain_id)?;
    validate_digest(&evidence.cache_format_id)?;
    validate_digest(&evidence.driver_id)?;
    validate_digest(&evidence.abi_id)?;
    let platform_inputs = PlatformInputs {
        os: platform.os.clone(),
        arch: platform.arch.clone(),
        runs_on: platform.runs_on.clone(),
        image_os: platform.image_os.clone(),
        image_version: platform.image_version.clone(),
        target: platform.target.clone(),
    };
    let _ = platform_id(&platform_inputs)?;
    Ok(digest_b3(&canonical_json_bytes(evidence)?))
}
