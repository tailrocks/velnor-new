//! Pure structural consistency; remote authority remains the caller's duty.

use std::collections::BTreeSet;

use super::{
    PreviewProductionContext, PreviewProductionRequest, PreviewProductionStage,
    expected_preview_payload_pairs, invalid, preview_plan_sha256,
};
use crate::{ContractError, PackageReleasePlan, preview_identity::PreviewIdentity};

pub(super) fn validate(request: &PreviewProductionRequest) -> Result<(), ContractError> {
    if request.schema != super::PREVIEW_PRODUCTION_SCHEMA_V1 {
        return Err(invalid("unknown_schema"));
    }
    let pairs = expected_preview_payload_pairs(&request.plan)?;
    if !lower_hex(&request.plan_sha256, 64)
        || request.plan_sha256 != preview_plan_sha256(&request.plan)?
    {
        return Err(invalid("full_plan_digest_mismatch"));
    }
    validate_context(&request.context, &request.plan)?;
    match &request.stage {
        PreviewProductionStage::Payload { pair } => {
            if pairs.binary_search(pair).is_err() {
                return Err(invalid("unknown_payload_pair"));
            }
        }
        PreviewProductionStage::Assemble { inputs } => {
            if inputs.len() != pairs.len()
                || inputs
                    .iter()
                    .zip(&pairs)
                    .any(|(input, pair)| &input.pair != pair)
            {
                return Err(invalid("incomplete_or_noncanonical_pair_inventory"));
            }
            let mut ids = BTreeSet::new();
            for input in inputs {
                if input.artifact_id == 0
                    || !ids.insert(input.artifact_id)
                    || !lower_hex(&input.sha256, 64)
                {
                    return Err(invalid("invalid_artifact_identity"));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn validate_context(
    context: &PreviewProductionContext,
    plan: &PackageReleasePlan,
) -> Result<(), ContractError> {
    if context.repository_id == 0
        || context.head_repository_id != context.repository_id
        || !lower_hex(&context.source_sha, 40)
        || context.source_ref != format!("refs/heads/{}", plan.default_branch)
        || context.workflow_authority_sha != context.source_sha
        || context.run_id == 0
        || context.run_attempt == 0
        || context.ci_run_id == 0
        || context.ci_run_attempt == 0
    {
        return Err(invalid("invalid_execution_context"));
    }
    let identity = PreviewIdentity::derive(
        &plan.base_version,
        &context.source_sha,
        context.commit_count,
    )?;
    if context.version != identity.version {
        return Err(invalid("derived_version_mismatch"));
    }
    Ok(())
}

fn lower_hex(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
