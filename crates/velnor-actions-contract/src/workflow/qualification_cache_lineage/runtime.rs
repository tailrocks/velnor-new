// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use super::receipt::{QualificationCacheBackendEntry, QualificationCacheBackendObservation};
use crate::canonical::{canonical_json_bytes, digest_b3, validate_digest};
use crate::errors::ContractError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationRuntimeIdentityField {
    RunnerOs,

    RunnerArch,

    ImageOs,

    ToolchainId,

    CacheFormatId,

    AbiId,

    DriverId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimePlatform {
    pub os: String,

    pub arch: String,

    pub runs_on: String,

    pub image_os: String,

    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeIdentity {
    pub platform: QualificationRuntimePlatform,

    pub toolchain_id: String,

    pub cache_format_id: String,

    pub abi_id: String,

    pub driver_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeIdentityRequirements {
    pub runner_label: String,

    pub target: String,

    pub toolchain_id: String,

    pub cache_format_id: String,

    pub require_abi: bool,

    pub driver_id: String,

    pub required_fields: Vec<QualificationRuntimeIdentityField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheRestoreExpectation {
    pub requested_key: String,

    pub expected_cache: Option<QualificationCacheBackendEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundQualificationCacheKeys {
    pub restore: Option<QualificationCacheRestoreExpectation>,

    pub save_key: Option<String>,

    pub save_if_state_changes: bool,

    pub expected_prior_state_digest: Option<String>,

    pub runtime_identity_digest: Option<String>,
}

impl BoundQualificationCacheKeys {
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
        target: platform.target.clone(),
    };
    let _ = platform_id(&platform_inputs)?;
    Ok(digest_b3(&canonical_json_bytes(evidence)?))
}
