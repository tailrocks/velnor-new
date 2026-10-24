//! Receipt shape and trusted-run provenance checks.

use crate::canonical::validate_digest;
use crate::errors::ContractError;
use crate::workflow::QualificationPhase;
use crate::workflow::plan::Plan;

use super::super::super::MAX_QUALIFICATION_CACHE_LANES;
use super::super::errors::{check_text, invalid};
use super::super::source_delta::validate_source_delta;
use super::super::types::{
    AdmissionNode, MAX_QUALIFICATION_RECEIPT_BYTES, QualificationCacheLaneReceipt,
    QualificationCacheOwnerReceipt, QualificationCacheProducerContext,
    QualificationCacheProducerEvidence, QualificationCacheReceipt, QualificationCacheReceiptLink,
    QualificationSourceDelta, qualification_cache_receipt_artifact_name,
};

struct ReceiptShape<'a> {
    schema: u32,
    plan_id: &'a str,
    campaign: &'a str,
    phase: QualificationPhase,
    source_sha: &'a str,
    configuration_digest: &'a str,
    source_delta: Option<&'a QualificationSourceDelta>,
    predecessor: Option<&'a QualificationCacheReceiptLink>,
    lanes: &'a [QualificationCacheLaneReceipt],
    owners: &'a [QualificationCacheOwnerReceipt],
    run: crate::workflow::QualificationRunRef,
}

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
    let branch = dispatch_branch(context)?;
    let workflow_path = dispatch_workflow_path(context)?;
    let workflow_path_ref = format!("{workflow_path}@{branch}");
    if producer.repository != metadata.repository
        || producer.default_branch != metadata.default_branch
        || producer.git_ref != metadata.git_ref
        || producer.mode != context.mode
        || producer.ref_protected != context.ref_protected
        || metadata.ref_protected != context.ref_protected
        || producer.workflow_ref != metadata.workflow_ref
        || producer.workflow_sha != metadata.workflow_sha
        || producer.source_sha != metadata.head_sha
        || producer.source_sha != metadata.workflow_sha
        || producer.run != metadata.run
        || node.receipt.source_sha != producer.source_sha
        || node.receipt.campaign != context.campaign
        || metadata.repository != context.repository
        || metadata.default_branch != context.default_branch
        || metadata.git_ref != context.git_ref
        || metadata.event != "workflow_dispatch"
        || metadata.conclusion != "success"
        || metadata.workflow_path_ref != workflow_path_ref
        || metadata.workflow_ref != context.workflow_ref
        || metadata.run != node.receipt.run
        || node.artifact.workflow_run_id != node.receipt.run.run_id
        || node.artifact.workflow_head_branch != branch
        || node.artifact.workflow_head_sha != node.receipt.source_sha
    {
        return Err(invalid("run_provenance_mismatch"));
    }
    Ok(())
}

fn dispatch_branch(
    context: &crate::workflow::QualificationDispatch,
) -> Result<&str, ContractError> {
    context
        .git_ref
        .strip_prefix("refs/heads/")
        .filter(|branch| crate::is_valid_branch_name(branch))
        .ok_or_else(|| invalid("dispatch_ref_invalid"))
}

fn dispatch_workflow_path(
    context: &crate::workflow::QualificationDispatch,
) -> Result<&str, ContractError> {
    let prefix = format!("{}/", context.repository);
    context
        .workflow_ref
        .strip_prefix(&prefix)
        .and_then(|suffix| suffix.split_once('@').map(|(path, _)| path))
        .filter(|path| {
            *path == crate::workflow::CI_WORKFLOW_PATH
                || *path == crate::workflow::QUALIFICATION_WORKFLOW_PATH
        })
        .ok_or_else(|| invalid("dispatch_workflow_invalid"))
}

pub(in crate::workflow::qualification_cache_lineage::receipt) fn validate_metadata(
    node: &AdmissionNode,
) -> Result<(), ContractError> {
    node.metadata.run.validate()?;
    node.producer.run.validate()?;
    node.receipt.run.validate()?;
    if node.artifact.id == 0
        || node.artifact.name != qualification_cache_receipt_artifact_name(node.receipt.run)?
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
    validate_receipt_parts(&ReceiptShape {
        schema: receipt.schema,
        plan_id: &receipt.plan_id,
        campaign: &receipt.campaign,
        phase: receipt.phase,
        source_sha: &receipt.source_sha,
        configuration_digest: &receipt.configuration_digest,
        source_delta: receipt.source_delta.as_ref(),
        predecessor: receipt.predecessor.as_ref(),
        lanes: &receipt.lanes,
        owners: &receipt.owners,
        run: receipt.run,
    })
}

pub(in crate::workflow::qualification_cache_lineage::receipt) fn validate_producer_evidence_shape(
    evidence: &QualificationCacheProducerEvidence,
    producer: &QualificationCacheProducerContext,
) -> Result<(), ContractError> {
    validate_receipt_parts(&ReceiptShape {
        schema: 4,
        plan_id: &evidence.plan_id,
        campaign: &evidence.campaign,
        phase: evidence.phase,
        source_sha: &evidence.source_sha,
        configuration_digest: &evidence.configuration_digest,
        source_delta: evidence.source_delta.as_ref(),
        predecessor: evidence.predecessor.as_ref(),
        lanes: &evidence.lanes,
        owners: &evidence.owners,
        run: producer.run,
    })
}

fn validate_receipt_parts(parts: &ReceiptShape<'_>) -> Result<(), ContractError> {
    if parts.schema != 4
        || parts.lanes.len() > MAX_QUALIFICATION_CACHE_LANES
        || parts.lanes.is_empty()
        || parts.owners.is_empty()
    {
        return Err(invalid("receipt_schema_or_lane_count"));
    }
    parts.run.validate()?;
    crate::ids::validate_plan_id(parts.plan_id)?;
    let run_key = crate::ids::run_key_for_ci(parts.run.run_id, u64::from(parts.run.run_attempt));
    if crate::ids::plan_id_for_run(&run_key)? != parts.plan_id {
        return Err(invalid("receipt_plan_id_run_mismatch"));
    }
    validate_digest(parts.configuration_digest)?;
    check_text(parts.campaign)?;
    validate_source_sha(parts.source_sha)?;
    if let Some(link) = parts.predecessor {
        link.run.validate()?;
        validate_digest(&link.receipt_digest)?;
        if link.artifact_id == 0 || !valid_sha256(&link.artifact_digest) {
            return Err(invalid("receipt_artifact_link_invalid"));
        }
    }
    match (parts.phase, parts.source_delta) {
        (QualificationPhase::UsefulDelta, Some(delta)) => {
            validate_source_delta(delta, &delta.base_source_sha, parts.source_sha)?;
        }
        (QualificationPhase::UsefulDelta, None) => {
            return Err(invalid("missing_useful_source_delta"));
        }
        (_, Some(_)) => return Err(invalid("unexpected_source_delta")),
        (_, None) => {}
    }
    for pair in parts.lanes.windows(2) {
        if pair[0].matrix_key >= pair[1].matrix_key {
            return Err(invalid("lanes_not_sorted_unique"));
        }
    }
    for pair in parts.owners.windows(2) {
        if pair[0].owner_id >= pair[1].owner_id {
            return Err(invalid("owners_not_sorted_unique"));
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
