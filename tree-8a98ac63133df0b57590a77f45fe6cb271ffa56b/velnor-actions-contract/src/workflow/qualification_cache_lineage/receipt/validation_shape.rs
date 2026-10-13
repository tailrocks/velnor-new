//! Receipt shape and trusted-run provenance checks.

use crate::canonical::validate_digest;
use crate::errors::ContractError;
use crate::workflow::QualificationPhase;
use crate::workflow::plan::Plan;

use super::super::super::MAX_QUALIFICATION_CACHE_LANES;
use super::super::errors::{check_text, invalid};
use super::super::source_delta::validate_source_delta;
use super::super::types::{
    AdmissionNode, MAX_QUALIFICATION_RECEIPT_BYTES, QUALIFICATION_CACHE_RECEIPT_ARTIFACT,
    QualificationCacheReceipt,
};

pub(in crate::workflow::qualification_cache_lineage::receipt) fn validate_node_metadata(
    plan: &Plan,
    node: &AdmissionNode,
) -> Result<(), ContractError> {
    let context = plan
        .qualification
        .as_ref()
        .ok_or_else(|| invalid("missing_dispatch"))?;
    let producer = &node.producer;
    let metadata = &node.metadata;
    if producer.repository != metadata.repository
        || producer.default_branch != metadata.default_branch
        || producer.git_ref != metadata.git_ref
        || !producer.ref_protected
        || !metadata.ref_protected
        || producer.workflow_ref != metadata.workflow_ref
        || producer.workflow_sha != metadata.workflow_sha
        || producer.source_sha != metadata.head_sha
        || producer.source_sha != metadata.workflow_sha
        || producer.run != metadata.run
        || node.receipt.source_sha != producer.source_sha
        || node.receipt.campaign != context.campaign
        || metadata.repository != context.repository
        || metadata.default_branch != context.default_branch
        || metadata.git_ref != format!("refs/heads/{}", context.default_branch)
        || metadata.event != "workflow_dispatch"
        || metadata.conclusion != "success"
        || metadata.workflow_path_ref
            != format!(
                "{}@{}",
                crate::workflow::CI_WORKFLOW_PATH,
                context.default_branch
            )
        || metadata.workflow_ref
            != format!(
                "{}/{}@refs/heads/{}",
                context.repository,
                crate::workflow::CI_WORKFLOW_PATH,
                context.default_branch
            )
        || metadata.run != node.receipt.run
        || node.artifact.workflow_run_id != node.receipt.run.run_id
        || node.artifact.workflow_head_branch != context.default_branch
        || node.artifact.workflow_head_sha != node.receipt.source_sha
    {
        return Err(invalid("run_provenance_mismatch"));
    }
    Ok(())
}

pub(in crate::workflow::qualification_cache_lineage::receipt) fn validate_metadata(
    node: &AdmissionNode,
) -> Result<(), ContractError> {
    node.metadata.run.validate()?;
    node.producer.run.validate()?;
    node.receipt.run.validate()?;
    if node.artifact.id == 0
        || node.artifact.name != QUALIFICATION_CACHE_RECEIPT_ARTIFACT
        || node.artifact.expired
        || node.artifact.size_bytes == 0
        || node.artifact.size_bytes > MAX_QUALIFICATION_RECEIPT_BYTES as u64
        || node.artifact.workflow_run_id == 0
        || !valid_sha256(&node.artifact.digest)
    {
        return Err(invalid("artifact_metadata_invalid"));
    }
    for value in [
        node.metadata.repository.as_str(),
        node.metadata.default_branch.as_str(),
        node.metadata.git_ref.as_str(),
        node.metadata.workflow_path_ref.as_str(),
        node.metadata.workflow_ref.as_str(),
        node.metadata.workflow_sha.as_str(),
        node.metadata.head_sha.as_str(),
        node.metadata.event.as_str(),
        node.metadata.conclusion.as_str(),
        node.artifact.name.as_str(),
        node.artifact.workflow_head_branch.as_str(),
        node.artifact.workflow_head_sha.as_str(),
        node.producer.repository.as_str(),
        node.producer.default_branch.as_str(),
        node.producer.git_ref.as_str(),
        node.producer.workflow_ref.as_str(),
        node.producer.workflow_sha.as_str(),
        node.producer.source_sha.as_str(),
    ] {
        check_text(value)?;
    }
    validate_source_sha(&node.metadata.workflow_sha)?;
    validate_source_sha(&node.metadata.head_sha)?;
    validate_source_sha(&node.producer.workflow_sha)?;
    validate_source_sha(&node.producer.source_sha)?;
    Ok(())
}

pub(in crate::workflow::qualification_cache_lineage::receipt) fn validate_receipt_shape(
    receipt: &QualificationCacheReceipt,
) -> Result<(), ContractError> {
    if receipt.schema != 1
        || receipt.lanes.len() > MAX_QUALIFICATION_CACHE_LANES
        || receipt.lanes.is_empty()
    {
        return Err(invalid("receipt_schema_or_lane_count"));
    }
    receipt.run.validate()?;
    crate::ids::validate_plan_id(&receipt.plan_id)?;
    let run_key =
        crate::ids::run_key_for_ci(receipt.run.run_id, u64::from(receipt.run.run_attempt));
    if crate::ids::plan_id_for_run(&run_key)? != receipt.plan_id {
        return Err(invalid("receipt_plan_id_run_mismatch"));
    }
    validate_digest(&receipt.configuration_digest)?;
    check_text(&receipt.campaign)?;
    validate_source_sha(&receipt.source_sha)?;
    if let Some(link) = &receipt.predecessor {
        link.run.validate()?;
        validate_digest(&link.receipt_digest)?;
        if link.artifact_id == 0 || !valid_sha256(&link.artifact_digest) {
            return Err(invalid("receipt_artifact_link_invalid"));
        }
    }
    match (receipt.phase, receipt.source_delta.as_ref()) {
        (QualificationPhase::UsefulDelta, Some(delta)) => {
            validate_source_delta(delta, &delta.base_source_sha, &receipt.source_sha)?;
        }
        (QualificationPhase::UsefulDelta, None) => {
            return Err(invalid("missing_useful_source_delta"));
        }
        (_, Some(_)) => return Err(invalid("unexpected_source_delta")),
        (_, None) => {}
    }
    for pair in receipt.lanes.windows(2) {
        if pair[0].matrix_key >= pair[1].matrix_key {
            return Err(invalid("lanes_not_sorted_unique"));
        }
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_source_sha(value: &str) -> Result<(), ContractError> {
    if value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(invalid("receipt_source_sha_invalid"))
    }
}
