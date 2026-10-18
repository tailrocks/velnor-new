use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;
use crate::workflow::plan::{MatrixEntry, Plan};

use super::{
    QualificationCacheLayer, QualificationRuntimeIdentityField,
    QualificationRuntimeIdentityRequirements, canonical_adapter_metadata,
};

/// Runtime identity requirements derived from validated plan identity.
pub(crate) fn runtime_requirements(
    plan: &Plan,
    entry: &MatrixEntry,
) -> Result<Option<QualificationRuntimeIdentityRequirements>, ContractError> {
    let Some(ids) = entry.cache_ids.as_ref() else {
        validate_uncached_identity(entry)?;
        return Ok(None);
    };
    ids.validate()?;
    let target = crate::ReleaseTarget::for_runner_label(&plan.runner.label)
        .map(crate::ReleaseTarget::triple)
        .ok_or_else(|| ContractError::identity("qualification.runner", "unsupported_target"))?;
    let runs_on = crate::RunsOn::parse(&entry.planned_platform.runs_on)?;
    let scale_set_image_source_sha = if runs_on.is_scale_set() {
        plan.qualification
            .as_ref()
            .and_then(|context| context.scale_set_image_source_sha.clone())
    } else {
        None
    };
    let toolchain_inputs = entry
        .toolchain_inputs
        .clone()
        .ok_or_else(|| ContractError::identity("matrix.toolchain_inputs", "missing"))?;
    let computed_toolchain_id = crate::cachekey::toolchain_id(&toolchain_inputs)?;
    if computed_toolchain_id != ids.toolchain_id()
        || entry
            .toolchain_id
            .as_deref()
            .is_some_and(|toolchain_id| toolchain_id != computed_toolchain_id)
    {
        return Err(ContractError::identity(
            "matrix.toolchain_inputs",
            "toolchain_id_mismatch",
        ));
    }
    Ok(Some(QualificationRuntimeIdentityRequirements {
        runner_label: plan.runner.label.clone(),
        runs_on: entry.planned_platform.runs_on.clone(),
        scale_set_image_source_sha,
        target: target.to_owned(),
        toolchain_id: ids.toolchain_id().to_owned(),
        toolchain_inputs,
        cache_format_id: ids.cache_format_id().to_owned(),
        require_abi: true,
        driver_id: digest_b3(&canonical_json_bytes(&canonical_adapter_metadata(
            plan,
            &entry.adapter_metadata,
        )?)?),
        required_fields: vec![
            QualificationRuntimeIdentityField::RunnerOs,
            QualificationRuntimeIdentityField::RunnerArch,
            QualificationRuntimeIdentityField::ImageIdentity,
            QualificationRuntimeIdentityField::ToolchainId,
            QualificationRuntimeIdentityField::CacheFormatId,
            QualificationRuntimeIdentityField::AbiId,
            QualificationRuntimeIdentityField::DriverId,
        ],
    }))
}

fn validate_uncached_identity(entry: &MatrixEntry) -> Result<(), ContractError> {
    if [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
    .into_iter()
    .any(|layer| super::super::receipt::layer_applies(entry, layer))
    {
        return Err(ContractError::identity(
            "qualification.cache_ids",
            "missing_for_active_layer",
        ));
    }
    let toolchain_inputs = entry
        .toolchain_inputs
        .as_ref()
        .ok_or_else(|| ContractError::identity("matrix.toolchain_inputs", "missing"))?;
    let toolchain_id = entry
        .toolchain_id
        .as_deref()
        .ok_or_else(|| ContractError::identity("matrix.toolchain_id", "missing"))?;
    if crate::cachekey::toolchain_id(toolchain_inputs)? != toolchain_id {
        return Err(ContractError::identity(
            "matrix.toolchain_id",
            "toolchain_inputs_mismatch",
        ));
    }
    Ok(())
}
