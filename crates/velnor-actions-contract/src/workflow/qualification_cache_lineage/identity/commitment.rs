use serde::Serialize;

use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;
use crate::workflow::plan::{MatrixEntry, Plan};

use super::{QualificationCacheLayer, canonical_adapter_metadata, configuration_commitment};

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

#[derive(Serialize)]
struct DisabledLayerIdentityPreimage<'a> {
    schema: u32,
    configuration_digest: &'a str,
    matrix_key: &'a str,
    layer: QualificationCacheLayer,
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
    let configuration_digest = configuration_commitment(plan)?;
    let Some(ids) = entry.cache_ids.as_ref() else {
        if super::super::receipt::layer_applies(entry, layer) {
            return Err(ContractError::identity(
                "qualification.cache_ids",
                "missing_for_active_layer",
            ));
        }
        let preimage = DisabledLayerIdentityPreimage {
            schema: 1,
            configuration_digest: &configuration_digest,
            matrix_key: &entry.matrix_key,
            layer,
        };
        return Ok(digest_b3(&canonical_json_bytes(&preimage)?));
    };
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
        configuration_digest: &configuration_digest,
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
