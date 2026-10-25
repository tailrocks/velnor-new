use std::error::Error;

use super::*;
use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::workflow::{
    QualificationCacheArtifact, QualificationCacheBackendEntry,
    QualificationCacheBackendObservation, QualificationCacheLaneLayerReceipt,
    QualificationCacheLaneReceipt, QualificationCacheLayer, QualificationCacheLayerReceipt,
    QualificationCacheOwnerDirective, QualificationCacheOwnerReceipt,
    QualificationCacheProducerEvidence, QualificationCacheReceipt,
    QualificationCacheReceiptArtifactDocument, QualificationCacheReceiptLink,
    QualificationCacheRestore, QualificationCacheRestoreResult, QualificationCacheSave,
    QualificationCacheSaveActionResult, QualificationPhase, QualificationRunRef,
    QualificationSourceDelta,
};

use super::directive_tests::{qualification_plan, runtime_identity};

#[path = "qualification_cache_lineage_metadata_fixture.rs"]
mod metadata_fixture;
use self::metadata_fixture::{
    artifact_metadata, dispatch, producer_context, run_metadata, run_ref,
};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

pub(super) struct BuiltNode {
    pub(super) value: serde_json::Value,
    pub(super) artifact: QualificationCacheArtifact,
    pub(super) document: QualificationCacheReceiptArtifactDocument,
    pub(super) receipt: QualificationCacheReceipt,
}

pub(super) fn phase_plan(
    phase: QualificationPhase,
    run_id: u64,
    source_sha: &str,
    predecessor_run_id: u64,
) -> Result<crate::workflow::Plan, crate::ContractError> {
    qualification_plan(
        phase,
        run_id,
        source_sha,
        Some(QualificationRunRef {
            run_id: predecessor_run_id,
            run_attempt: 1,
        }),
    )
}

pub(super) fn completed_node(
    plan: &crate::workflow::Plan,
    previous: Option<&BuiltNode>,
    source_delta: Option<QualificationSourceDelta>,
) -> TestResult<BuiltNode> {
    let previous_admission = previous
        .map(|node| admission_for(node, source_delta.as_ref()))
        .transpose()?;
    let producer = producer_context(plan)?;
    let metadata = run_metadata(plan)?;
    let receipt = completed_receipt(plan, previous_admission.as_ref(), previous, source_delta)?;
    let document = QualificationCacheReceiptArtifactDocument {
        schema: 4,
        producer,
        evidence: QualificationCacheProducerEvidence {
            plan_id: receipt.plan_id.clone(),
            campaign: receipt.campaign.clone(),
            phase: receipt.phase,
            source_sha: receipt.source_sha.clone(),
            configuration_digest: receipt.configuration_digest.clone(),
            source_delta: receipt.source_delta.clone(),
            predecessor: receipt.predecessor.clone(),
            lanes: receipt.lanes.clone(),
            owners: receipt.owners.clone(),
        },
    };
    let bytes = document.to_bounded_json(plan, previous_admission.as_ref())?;
    let artifact = artifact_metadata(plan, bytes.len())?;
    let value = serde_json::json!({
        "metadata": metadata,
        "artifact": artifact.clone(),
        "producer": document.producer.clone(),
        "receipt": receipt.clone(),
        "previous": previous.map(|node| node.value.clone()),
    });
    Ok(BuiltNode {
        value,
        artifact,
        document,
        receipt,
    })
}

fn completed_receipt(
    plan: &crate::workflow::Plan,
    admission: Option<&QualificationCacheAdmission>,
    previous: Option<&BuiltNode>,
    source_delta: Option<QualificationSourceDelta>,
) -> TestResult<QualificationCacheReceipt> {
    let context = dispatch(plan)?;
    let directive = QualificationCacheDirective::for_plan(plan, admission)?
        .ok_or_else(|| std::io::Error::other("qualification directive missing"))?;
    let lanes = plan
        .matrix
        .include
        .iter()
        .map(|entry| lane_receipt(plan, entry, &directive, admission))
        .collect::<TestResult<Vec<_>>>()?;
    let predecessor = previous.map(receipt_link).transpose()?;
    Ok(QualificationCacheReceipt {
        schema: 4,
        plan_id: plan.plan_id.clone(),
        run: run_ref(context),
        campaign: context.campaign.clone(),
        phase: context.phase,
        source_sha: context.source_sha.clone(),
        configuration_digest: super::identity::configuration_commitment(plan)?,
        source_delta,
        predecessor,
        lanes,
        owners: owner_receipts(plan, &directive, admission)?,
    })
}

#[path = "qualification_cache_lineage_admission_fixture/receipts.rs"]
mod receipts;
use self::receipts::{lane_receipt, owner_receipts, receipt_link};

pub(super) fn admission_for(
    node: &BuiltNode,
    source_delta: Option<&QualificationSourceDelta>,
) -> TestResult<QualificationCacheAdmission> {
    admission_from_value(&node.value, source_delta)
}

pub(super) fn admission_from_value(
    node: &serde_json::Value,
    source_delta: Option<&QualificationSourceDelta>,
) -> TestResult<QualificationCacheAdmission> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "predecessor": node,
        "source_delta": source_delta,
    }))?;
    Ok(QualificationCacheAdmission::parse_bounded(&bytes)?)
}

pub(super) fn useful_delta(base: &str, source: &str) -> TestResult<QualificationSourceDelta> {
    let changed_paths = vec!["crates/demo/src/lib.rs".to_owned()];
    Ok(QualificationSourceDelta {
        base_source_sha: base.to_owned(),
        source_sha: source.to_owned(),
        diff_digest: digest_b3(&canonical_json_bytes(&changed_paths)?),
        changed_paths,
        base_is_ancestor: true,
    })
}
