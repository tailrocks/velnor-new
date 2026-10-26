// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::{
    ReleaseProductionContext, ReleaseProductionRequest, ReleaseProductionStage,
    expected_release_payload_pairs, invalid, release_plan_sha256,
};
use crate::{ContractError, PackageReleasePlan, preview_identity::PreviewIdentity};

pub(super) fn validate(request: &ReleaseProductionRequest) -> Result<(), ContractError> {
    if request.schema != super::RELEASE_PRODUCTION_SCHEMA_V1 {
        return Err(invalid("unknown_schema"));
    }
    let pairs = expected_release_payload_pairs(&request.plan)?;
    if !lower_hex(&request.policy_sha256, 64)
        || !lower_hex(&request.plan_sha256, 64)
        || request.plan_sha256 != release_plan_sha256(&request.plan)?
    {
        return Err(invalid("full_plan_digest_mismatch"));
    }
    validate_context(&request.context, &request.plan)?;
    match &request.stage {
        ReleaseProductionStage::Payload { pair } => {
            if pairs.binary_search(pair).is_err() {
                return Err(invalid("unknown_payload_pair"));
            }
        }
        ReleaseProductionStage::Assemble { inputs } => {
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
    context: &ReleaseProductionContext,
    plan: &PackageReleasePlan,
) -> Result<(), ContractError> {
    if context.repository_id == 0
        || context.head_repository_id != context.repository_id
        || !lower_hex(&context.source_sha, 40)
        || !lower_hex(&context.workflow_authority_sha, 40)
        || context.source_ref != format!("refs/heads/{}", plan.default_branch)
        || (plan.channel == crate::package_release::PackageReleaseChannel::Stable
            && context.workflow_authority_sha != context.source_sha)
        || context.run_id == 0
        || context.run_attempt == 0
        || context.ci_run_id == 0
        || context.ci_run_attempt == 0
    {
        return Err(invalid("invalid_execution_context"));
    }
    let version = match plan.channel {
        crate::package_release::PackageReleaseChannel::Preview => {
            PreviewIdentity::derive(
                &plan.base_version,
                &context.source_sha,
                context.commit_count,
            )?
            .version
        }
        crate::package_release::PackageReleaseChannel::Stable => {
            if context.commit_count != 0 {
                return Err(invalid("stable_commit_count"));
            }
            plan.base_version.clone()
        }
    };
    if context.version != version {
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
