//! Static and runtime identity commitments for qualification cache keys.

use serde::{Deserialize, Serialize};

use crate::canonical::{canonical_json_bytes, digest_b3, validate_digest};
use crate::errors::ContractError;
use crate::workflow::plan::{MatrixEntry, Plan};

pub(crate) use super::runtime::validate_runtime_identity;
pub use super::runtime::{
    BoundQualificationCacheKeys, QualificationCacheRestoreExpectation,
    QualificationRuntimeIdentity, QualificationRuntimeIdentityField,
    QualificationRuntimeIdentityRequirements, QualificationRuntimePlatform,
};

/// Maximum selected matrix lanes represented in a qualification directive.
pub const MAX_QUALIFICATION_CACHE_LANES: usize = 256;

/// Closed set of cache transports used by the current V1 qualification path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

#[derive(Serialize)]
struct StaticMatrixEntry<'a> {
    id: &'a str,
    matrix_key: &'a str,
    stack_id: &'a str,
    task_id: &'a str,
    run: &'a str,
    task_digest: &'a str,
    adapter_metadata: serde_json::Value,
    execute_task_ids: &'a crate::workflow::ExecuteTaskIds,
    job_id: &'a str,
    cache_ids: &'a Option<crate::workflow::EntryCacheIds>,
    declared_outputs: &'a [String],
}

#[derive(Serialize)]
struct StaticObligation<'a> {
    task_id: &'a str,
    decision: crate::workflow::ObligationDecision,
    task_digest: &'a str,
}

#[derive(Serialize)]
struct StaticPackage<'a> {
    logical_id: String,
    name: &'a str,
    manifest: &'a str,
    selected: bool,
    reasons: &'a [String],
    tasks: &'a [String],
}

#[derive(Serialize)]
struct PlanConfiguration<'a> {
    runner: &'a crate::workflow::PlanRunner,
    generator: &'a crate::workflow::PlanGenerator,
    packages: Vec<StaticPackage<'a>>,
    obligations: Vec<StaticObligation<'a>>,
    task_ids: &'a [String],
    edges: &'a [crate::graph::TaskEdge],
    matrix: Vec<StaticMatrixEntry<'a>>,
}

#[derive(Serialize)]
struct IdentityPreimage<'a> {
    schema: u32,
    repository: &'a str,
    campaign: &'a str,
    configuration_digest: &'a str,
    runner_label: &'a str,
    target: &'a str,
    matrix_key: &'a str,
    stack_id: &'a str,
    task_id: &'a str,
    task_digest: &'a str,
    driver_id: String,
    layer: QualificationCacheLayer,
    workspace_id: &'a str,
    lane_id: &'a str,
    platform_id: &'a str,
    toolchain_id: &'a str,
    cache_format_id: &'a str,
}

/// Derive the stable configuration commitment shared by all runs in a campaign.
pub(crate) fn configuration_commitment(plan: &Plan) -> Result<String, ContractError> {
    let mut packages: Vec<_> = plan
        .packages
        .iter()
        .map(|package| StaticPackage {
            logical_id: logical_package_id(package),
            name: &package.name,
            manifest: &package.manifest,
            selected: package.selected,
            reasons: &package.reasons,
            tasks: &package.tasks,
        })
        .collect();
    packages.sort_by(|left, right| left.logical_id.cmp(&right.logical_id));
    if packages
        .windows(2)
        .any(|pair| pair[0].logical_id == pair[1].logical_id)
    {
        return Err(ContractError::identity(
            "qualification.configuration.packages",
            "logical_package_collision",
        ));
    }
    let obligations = plan
        .obligations
        .iter()
        .map(|obligation| StaticObligation {
            task_id: &obligation.task_id,
            decision: obligation.decision,
            task_digest: &obligation.task_digest,
        })
        .collect();
    let matrix = plan
        .matrix
        .include
        .iter()
        .map(|entry| {
            Ok(StaticMatrixEntry {
                id: &entry.id,
                matrix_key: &entry.matrix_key,
                stack_id: &entry.stack_id,
                task_id: &entry.task_id,
                run: &entry.run,
                task_digest: &entry.task_digest,
                adapter_metadata: canonical_adapter_metadata(plan, &entry.adapter_metadata)?,
                execute_task_ids: &entry.execute_task_ids,
                job_id: &entry.job_id,
                cache_ids: &entry.cache_ids,
                declared_outputs: &entry.declared_outputs,
            })
        })
        .collect::<Result<Vec<_>, ContractError>>()?;
    let configuration = PlanConfiguration {
        runner: &plan.runner,
        generator: &plan.generator,
        packages,
        obligations,
        task_ids: &plan.task_ids,
        edges: &plan.edges,
        matrix,
    };
    Ok(digest_b3(&canonical_json_bytes(&configuration)?))
}

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

/// Derive the stable identity commitment for one plan lane and cache layer.
pub(crate) fn identity_commitment(
    plan: &Plan,
    entry: &MatrixEntry,
    layer: QualificationCacheLayer,
) -> Result<String, ContractError> {
    let dispatch = plan
        .qualification
        .as_ref()
        .ok_or_else(|| ContractError::identity("qualification", "missing_dispatch_context"))?;
    let ids = entry
        .cache_ids
        .as_ref()
        .ok_or_else(|| ContractError::identity("qualification.cache_ids", "missing"))?;
    ids.validate()?;
    let target = crate::ReleaseTarget::for_runner_label(&plan.runner.label)
        .map(crate::ReleaseTarget::triple)
        .ok_or_else(|| ContractError::identity("qualification.runner", "unsupported_target"))?;
    let adapter_metadata = canonical_adapter_metadata(plan, &entry.adapter_metadata)?;
    let driver_id = digest_b3(&canonical_json_bytes(&adapter_metadata)?);
    let preimage = IdentityPreimage {
        schema: 1,
        repository: &dispatch.repository,
        campaign: &dispatch.campaign,
        configuration_digest: &configuration_commitment(plan)?,
        runner_label: &plan.runner.label,
        target,
        matrix_key: &entry.matrix_key,
        stack_id: &entry.stack_id,
        task_id: &entry.task_id,
        task_digest: &entry.task_digest,
        driver_id,
        layer,
        workspace_id: ids.workspace_id(),
        lane_id: ids.lane_id(),
        platform_id: ids.platform_id(),
        toolchain_id: ids.toolchain_id(),
        cache_format_id: ids.cache_format_id(),
    };
    Ok(digest_b3(&canonical_json_bytes(&preimage)?))
}

/// Runtime identity requirements derived from validated plan identity.
pub(crate) fn runtime_requirements(
    plan: &Plan,
    entry: &MatrixEntry,
) -> Result<QualificationRuntimeIdentityRequirements, ContractError> {
    let ids = entry
        .cache_ids
        .as_ref()
        .ok_or_else(|| ContractError::identity("qualification.cache_ids", "missing"))?;
    ids.validate()?;
    let target = crate::ReleaseTarget::for_runner_label(&plan.runner.label)
        .map(crate::ReleaseTarget::triple)
        .ok_or_else(|| ContractError::identity("qualification.runner", "unsupported_target"))?;
    Ok(QualificationRuntimeIdentityRequirements {
        runner_label: plan.runner.label.clone(),
        target: target.to_owned(),
        toolchain_id: ids.toolchain_id().to_owned(),
        cache_format_id: ids.cache_format_id().to_owned(),
        require_abi: true,
        driver_id: digest_b3(&canonical_json_bytes(&canonical_adapter_metadata(
            plan,
            &entry.adapter_metadata,
        )?)?),
        required_fields: vec![
            QualificationRuntimeIdentityField::RunnerOs,
            QualificationRuntimeIdentityField::RunnerArch,
            QualificationRuntimeIdentityField::ImageOs,
            QualificationRuntimeIdentityField::ImageVersion,
            QualificationRuntimeIdentityField::ToolchainId,
            QualificationRuntimeIdentityField::CacheFormatId,
            QualificationRuntimeIdentityField::AbiId,
            QualificationRuntimeIdentityField::DriverId,
        ],
    })
}

/// Construct a stable archive key from validated identity and one typed slot.
pub(crate) fn key_for_slot(
    campaign: &str,
    layer: QualificationCacheLayer,
    identity_digest: &str,
    runtime_digest: &str,
    slot: QualificationCacheSlot,
) -> Result<String, ContractError> {
    validate_digest(identity_digest)?;
    validate_digest(runtime_digest)?;
    let key = format!(
        "velnor-qcache-v1-{campaign}-{}-{identity_digest}-{runtime_digest}-{}",
        layer.as_str(),
        slot.as_str()
    );
    if campaign.is_empty()
        || campaign.len() > 64
        || !campaign
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || key.len() > crate::cachekey::MAX_CACHE_KEY_BYTES
    {
        return Err(ContractError::identity(
            "qualification.cache_key",
            "invalid_or_too_long",
        ));
    }
    Ok(key)
}

#[cfg(test)]
#[path = "../qualification_cache_lineage_identity_tests.rs"]
mod tests;
