//! Runtime identity evidence and exact cache key binding.

use serde::{Deserialize, Serialize};

use super::super::receipt::{QualificationCacheBackendEntry, QualificationCacheBackendObservation};
use crate::cachekey::ToolchainInputs;
use crate::canonical::validate_digest;
use crate::errors::ContractError;

/// Runtime fact a cache producer must collect before binding a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationRuntimeIdentityField {
    /// Actual hosted runner OS.
    RunnerOs,
    /// Actual hosted runner architecture.
    RunnerArch,
    /// Actual hosted image or verified Scale Set image identity.
    ImageIdentity,
    /// Toolchain identity observed after pinned tool setup.
    ToolchainId,
    /// Cache-format identity reported by its producer.
    CacheFormatId,
    /// ABI identity reported by the actual runner and toolchain.
    AbiId,
    /// Selected compile-driver/configuration identity.
    DriverId,
}

/// Digests for one verified container image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeImageDigests {
    /// SHA-256 digest of the source archive, as lowercase hex.
    pub archive_sha256: String,
    /// SHA-256 hash of the archive's outer `index.json`, as lowercase hex.
    pub archive_index_digest: String,
    /// Digest of a nested selected OCI image index, when one is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_index_digest: Option<String>,
    /// OCI manifest digest in `sha256:<hex>` form.
    pub manifest_digest: String,
    /// OCI config digest in `sha256:<hex>` form.
    pub config_digest: String,
}

/// Verified Scale Set labels and name published by the runner host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeScaleSet {
    /// Registered Scale Set name.
    pub name: String,
    /// Exact Scale Set selector labels in render order.
    pub labels: Vec<String>,
}

/// Image facts supplied by GitHub or the verified runner host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualificationRuntimeImageIdentity {
    /// Image family supplied by a GitHub-hosted runner.
    Hosted {
        /// Hosted image family.
        image_os: String,
    },
    /// Image data from the runner host's read-only guest projection.
    ScaleSet {
        /// Source revision used to build the verified guest images.
        source_sha: String,
        /// OCI guest platform, such as `linux/amd64`.
        guest_platform: String,
        /// Scale Set selector values verified by the runner host.
        scale_set: Box<QualificationRuntimeScaleSet>,
        /// Verified runner image digests.
        runner_image: Box<QualificationRuntimeImageDigests>,
        /// Verified Docker-in-Docker image digests.
        dind_image: Box<QualificationRuntimeImageDigests>,
    },
}

/// Runtime platform dimensions reported by the active runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimePlatform {
    /// Operating system name.
    pub os: String,
    /// CPU architecture.
    pub arch: String,
    /// Logical runner label from the plan.
    pub runner_label: String,
    /// Exact hosted label or canonical Scale Set selector.
    pub runs_on: String,
    /// Actual image facts for the selected runner.
    pub image_identity: QualificationRuntimeImageIdentity,
    /// Execution target (`host` or a triple).
    pub target: String,
}

/// One tool version and executable digest observed after trusted setup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeToolObservation {
    /// Exact pinned selector from the plan.
    pub selector: String,
    /// Version reported by the resolved executable.
    pub reported_version: String,
    /// SHA-256 of the resolved executable bytes, as lowercase hex.
    pub executable_sha256: String,
}

/// ABI or runtime facts collected from the selected host and toolchain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "family", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualificationRuntimeAbiObservation {
    /// Host kernel, architecture and C runtime facts.
    Host {
        /// Operating-system name reported by the host.
        os: String,
        /// Architecture reported by the host.
        arch: String,
        /// Kernel release reported by the host.
        kernel: String,
        /// C runtime release, when the operating system has one.
        libc: Option<String>,
    },
    /// Rust compiler host, target and cfg facts.
    Rust {
        /// Rust compiler's host triple.
        host: String,
        /// Exact selected compilation target.
        target: String,
        /// Output from `rustc --version --verbose`.
        version_verbose: String,
        /// Output from `rustc --print cfg --target <target>`.
        target_cfg: String,
        /// Installed Rustup components reported by `rustup component list`.
        installed_components: Vec<String>,
    },
    /// Java runtime version and machine architecture.
    Java {
        /// Runtime version output.
        version_output: String,
        /// Machine architecture reported by the runtime.
        arch: String,
    },
    /// JavaScript runtime and native module ABI facts.
    Node {
        /// Runtime version output.
        version_output: String,
        /// Native module ABI version.
        module_abi: String,
        /// N-API version.
        napi: String,
        /// Operating-system platform reported by the runtime.
        platform: String,
        /// Machine architecture reported by the runtime.
        arch: String,
    },
    /// `OpenTofu` runtime version and platform facts.
    OpenTofu {
        /// Version output.
        version_output: String,
        /// Operating-system and architecture reported by `OpenTofu`.
        platform: String,
    },
}

/// Runtime facts needed to bind a logical slot to one exact cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeIdentity {
    /// Runtime OS, architecture, label, image and target evidence.
    pub platform: QualificationRuntimePlatform,
    /// Exact tool versions and executable bytes observed after trusted setup.
    pub tools: Vec<QualificationRuntimeToolObservation>,
    /// ABI facts reported by the host and selected runtime adapters.
    pub abi: Vec<QualificationRuntimeAbiObservation>,
}

/// Plan-known cache identity values plus runtime facts still required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeIdentityRequirements {
    /// Logical runner label from the plan.
    pub runner_label: String,
    /// Exact hosted label or Scale Set selector from the plan lane.
    pub runs_on: String,
    /// Expected Velnor source revision for a Scale Set guest image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_set_image_source_sha: Option<String>,
    /// Execution target associated with that runner label.
    pub target: String,
    /// Toolchain digest recorded by the matrix entry.
    pub toolchain_id: String,
    /// Exact pins and driver inputs behind `toolchain_id`.
    pub toolchain_inputs: ToolchainInputs,
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
