// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::types::{
    MAX_DEPTH, MAX_LANES, MAX_RECEIPT_BYTES, Node, Phase, RECEIPT_ARTIFACT, Receipt,
};
use super::validate::{
    check_text, invalid, receipt_digest, valid_sha256, validate_digest, validate_plan_id,
    validate_source_delta, validate_source_sha,
};
use crate::error::CiError;

pub(super) fn node_shape(node: &Node, depth: usize) -> Result<(), CiError> {
    if depth >= MAX_DEPTH {
        return Err(invalid("lineage_too_deep"));
    }
    node_metadata(node)?;
    receipt_shape(&node.receipt)?;
    if let Some(previous) = &node.previous {
        let link = node
            .receipt
            .predecessor
            .as_ref()
            .ok_or_else(|| invalid("missing_predecessor_link"))?;
        if link.run != previous.receipt.run
            || link.receipt_digest != receipt_digest(&previous.receipt)?
            || link.artifact_id != previous.artifact.id
            || link.artifact_digest != previous.artifact.digest
        {
            return Err(invalid("predecessor_receipt_digest_mismatch"));
        }
        node_shape(previous, depth + 1)?;
    } else if node.receipt.predecessor.is_some() {
        return Err(invalid("missing_predecessor_receipt"));
    }
    Ok(())
}

fn node_metadata(node: &Node) -> Result<(), CiError> {
    node.metadata.run.validate()?;
    node.producer.run.validate()?;
    node.receipt.run.validate()?;
    let artifact = &node.artifact;
    if artifact.id == 0
        || artifact.name != RECEIPT_ARTIFACT
        || artifact.expired
        || artifact.size_bytes == 0
        || artifact.size_bytes > MAX_RECEIPT_BYTES as u64
        || artifact.workflow_run_id == 0
        || !valid_sha256(&artifact.digest)
    {
        return Err(invalid("artifact_metadata_invalid"));
    }
    let (meta, producer) = (&node.metadata, &node.producer);
    for value in [
        &meta.repository,
        &meta.default_branch,
        &meta.git_ref,
        &meta.workflow_path_ref,
        &meta.workflow_ref,
        &meta.workflow_sha,
        &meta.head_sha,
        &meta.event,
        &meta.conclusion,
        &artifact.name,
        &artifact.workflow_head_branch,
        &artifact.workflow_head_sha,
        &producer.repository,
        &producer.default_branch,
        &producer.git_ref,
        &producer.workflow_ref,
        &producer.workflow_sha,
        &producer.source_sha,
    ] {
        check_text(value)?;
    }
    for sha in [
        &meta.workflow_sha,
        &meta.head_sha,
        &producer.workflow_sha,
        &producer.source_sha,
    ] {
        validate_source_sha(sha)?;
    }
    Ok(())
}

fn receipt_shape(receipt: &Receipt) -> Result<(), CiError> {
    if receipt.schema != 2 || receipt.lanes.len() > MAX_LANES || receipt.lanes.is_empty() {
        return Err(invalid("receipt_schema_or_lane_count"));
    }
    receipt.run.validate()?;
    validate_plan_id(&receipt.plan_id)?;
    let run_key = format!("r{}-a{}", receipt.run.run_id, receipt.run.run_attempt);
    if format!("plan-{run_key}") != receipt.plan_id {
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
        (Phase::UsefulDelta, Some(delta)) => {
            validate_source_delta(delta, &delta.base_source_sha, &receipt.source_sha)?;
        }
        (Phase::UsefulDelta, None) => return Err(invalid("missing_useful_source_delta")),
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
