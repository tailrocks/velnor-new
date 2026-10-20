//! Static and runtime identity commitments for qualification cache keys.

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::workflow::plan::Plan;

pub(crate) use super::runtime::validate_runtime_identity;
pub use super::runtime::{
    BoundQualificationCacheKeys, QualificationCacheRestoreExpectation,
    QualificationRuntimeAbiObservation, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityField, QualificationRuntimeIdentityRequirements,
    QualificationRuntimeImageDigests, QualificationRuntimeImageIdentity,
    QualificationRuntimePlatform, QualificationRuntimeScaleSet,
    QualificationRuntimeToolObservation,
};

/// Maximum selected matrix lanes represented in a qualification directive.
pub const MAX_QUALIFICATION_CACHE_LANES: usize = 256;

/// Closed set of cache transports used by the current V1 qualification path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheLayer {
    /// MBX action-owned object store.
    MbxObjects,
    /// Shared Cargo registry and Git sources archive.
    CargoSources,
    /// V2 Mise tools archive.
    MiseTools,
    /// `OpenTofu` provider archive.
    TofuProviders,
    /// Task-result cache, disabled for qualification.
    TaskResult,
}

impl QualificationCacheLayer {
    /// Stable wire and key-layer spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MbxObjects => "mbx_objects",
            Self::CargoSources => "cargo_sources",
            Self::MiseTools => "mise_tools",
            Self::TofuProviders => "tofu_providers",
            Self::TaskResult => "task_result",
        }
    }
}

/// Cache-generation slot within one isolated campaign and identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum QualificationCacheSlot {
    /// Cold seed snapshot.
    #[serde(rename = "k1")]
    K1,
    /// Warm persisted snapshot.
    #[serde(rename = "k2")]
    K2,
    /// Useful-delta snapshot.
    #[serde(rename = "k3")]
    K3,
}

impl QualificationCacheSlot {
    /// Stable lowercase slot spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::K1 => "k1",
            Self::K2 => "k2",
            Self::K3 => "k3",
        }
    }
}

mod cache_key;
mod commitment;
mod configuration;
mod owner;
mod runtime_requirements;

pub(crate) use cache_key::key_for_slot;
pub(crate) use commitment::identity_commitment;
pub(crate) use configuration::configuration_commitment;
pub use owner::QualificationCacheOwnerMemberRef;
pub(crate) use owner::{
    QualificationCacheOwner, owner_for_plan, owner_layer_identity, owner_runtime_requirements,
};
pub(crate) use runtime_requirements::runtime_requirements;

fn logical_package_id(package: &crate::workflow::PlanPackage) -> String {
    format!(
        "{}#{}",
        package.manifest,
        crate::component_id_for_unit(&package.package_id, &package.manifest)
    )
}

fn canonical_adapter_metadata(
    plan: &Plan,
    metadata: &serde_json::Value,
) -> Result<serde_json::Value, ContractError> {
    let mut canonical = metadata.clone();
    let Some(object) = canonical.as_object_mut() else {
        return Ok(canonical);
    };
    for field in ["package_id", "unit_id"] {
        let Some(value) = object.get(field).and_then(serde_json::Value::as_str) else {
            continue;
        };
        let package = plan
            .packages
            .iter()
            .find(|package| package.package_id == value)
            .ok_or_else(|| {
                ContractError::identity(
                    "qualification.configuration.package_id",
                    "not_in_plan_inventory",
                )
            })?;
        object.insert(
            field.to_owned(),
            serde_json::Value::String(logical_package_id(package)),
        );
    }
    Ok(canonical)
}

#[cfg(test)]
#[path = "../qualification_cache_lineage_identity_tests.rs"]
mod tests;
