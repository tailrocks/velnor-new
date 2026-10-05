//! Immutable predecessor fetch, ZIP verification, and recursive receipt binding.

use serde_json::Value;
use velnor_actions_contract::workflow::{
    QualificationCacheArtifact, QualificationCacheProducerContext,
    QualificationCacheReceiptArtifactDocument, QualificationCacheReceiptLink,
    QualificationCacheRunMetadata, QualificationPhase, QualificationRunRef,
};

use crate::OrchestratorError;
use crate::internal::internal;
use crate::qualification_github_api::{
    QualificationGitHubApi, RECEIPT_ARTIFACT_NAME, RunApiRecord,
};
use crate::qualification_receipt_archive::verify_and_extract;

/// Maximum linked historical runs before a receipt chain is rejected.
const MAX_PREDECESSOR_NODES: usize = 3;
/// Exact workflow path accepted from the run API.
const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";

/// Fetch one attempt, its unique artifact, and all linked earlier receipts.
#[expect(
    clippy::too_many_arguments,
    reason = "one recursion carries every independently validated lineage binding"
)]
pub(super) fn resolve_node(
    api: &QualificationGitHubApi,
    default_branch: &str,
    repository: &str,
    context: &velnor_actions_contract::QualificationDispatch,
    run_ref: QualificationRunRef,
    expected_phase: QualificationPhase,
    artifact_link: Option<&QualificationCacheReceiptLink>,
    depth: usize,
) -> Result<Value, OrchestratorError> {
    if depth > MAX_PREDECESSOR_NODES {
        return Err(internal("qualification_receipt_lineage_too_deep"));
    }
    let run = api.run_attempt(run_ref.run_id, run_ref.run_attempt, default_branch)?;
    validate_predecessor_run(&run, repository, default_branch)?;
    let artifact = api.receipt_artifact(run_ref.run_id, RECEIPT_ARTIFACT_NAME)?;
    validate_artifact_link(&artifact, run_ref.run_id, &run, artifact_link)?;
    let document = download_document(api, &artifact)?;
    validate_producer_and_receipt(
        &document,
        &run,
        repository,
        default_branch,
        run_ref,
        expected_phase,
        context,
    )?;
    let previous = resolve_previous(
        api,
        default_branch,
        repository,
        context,
        &document.receipt.predecessor,
        expected_phase,
        depth,
    )?;
    let metadata = metadata_for(&run, &document.producer)?;
    let artifact_document = QualificationCacheArtifact {
        id: artifact.id,
        name: artifact.name,
        digest: artifact.digest,
        size_bytes: artifact.size_bytes,
        expired: artifact.expired,
        workflow_run_id: artifact.workflow_run_id,
        workflow_head_branch: artifact.workflow_head_branch,
        workflow_head_sha: artifact.workflow_head_sha,
    };
    Ok(serde_json::json!({
        "metadata": metadata,
        "artifact": artifact_document,
        "producer": document.producer,
        "receipt": document.receipt,
        "previous": previous
    }))
}

/// Source SHA for the immediate receipt in one validated node value.
pub(super) fn node_source_sha(node: &Value) -> Result<String, OrchestratorError> {
    node["receipt"]["source_sha"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| internal("qualification_receipt_source_missing"))
}

fn validate_predecessor_run(
    run: &RunApiRecord,
    repository: &str,
    default_branch: &str,
) -> Result<(), OrchestratorError> {
    if run.repository != repository
        || run.head_branch != default_branch
        || run.event != "workflow_dispatch"
        || run.status != "completed"
        || run.conclusion.as_deref() != Some("success")
        || run.path_ref != format!("{CI_WORKFLOW_PATH}@{default_branch}")
    {
        return Err(internal("qualification_predecessor_run_invalid"));
    }
    Ok(())
}

fn validate_artifact_link(
    artifact: &crate::qualification_github_api::ArtifactApiRecord,
    run_id: u64,
    run: &RunApiRecord,
    link: Option<&QualificationCacheReceiptLink>,
) -> Result<(), OrchestratorError> {
    if artifact.name != RECEIPT_ARTIFACT_NAME
        || artifact.workflow_run_id != run_id
        || artifact.workflow_head_branch != run.head_branch
        || artifact.workflow_head_sha != run.head_sha
        || link.is_some_and(|value| {
            value.artifact_id != artifact.id || value.artifact_digest != artifact.digest
        })
    {
        return Err(internal("qualification_receipt_artifact_link_mismatch"));
    }
    Ok(())
}

fn download_document(
    api: &QualificationGitHubApi,
    artifact: &crate::qualification_github_api::ArtifactApiRecord,
) -> Result<QualificationCacheReceiptArtifactDocument, OrchestratorError> {
    let bytes = api.download_artifact(artifact.id)?;
    if u64::try_from(bytes.len()).ok() != Some(artifact.size_bytes) {
        return Err(internal("qualification_receipt_archive_size_mismatch"));
    }
    let document_bytes = verify_and_extract(&bytes, &artifact.digest)?;
    serde_json::from_slice(&document_bytes)
        .map_err(|_| internal("qualification_receipt_document_invalid"))
}

fn validate_producer_and_receipt(
    document: &QualificationCacheReceiptArtifactDocument,
    run: &RunApiRecord,
    repository: &str,
    default_branch: &str,
    run_ref: QualificationRunRef,
    phase: QualificationPhase,
    context: &velnor_actions_contract::QualificationDispatch,
) -> Result<(), OrchestratorError> {
    let producer = &document.producer;
    let receipt = &document.receipt;
    if document.schema != 1
        || producer.repository != repository
        || producer.default_branch != default_branch
        || producer.git_ref != format!("refs/heads/{default_branch}")
        || !producer.ref_protected
        || producer.workflow_ref
            != format!("{repository}/{CI_WORKFLOW_PATH}@refs/heads/{default_branch}")
        || producer.workflow_sha != run.head_sha
        || producer.source_sha != run.head_sha
        || producer.run != run_ref
        || receipt.run != run_ref
        || receipt.phase != phase
        || receipt.campaign != context.campaign
        || receipt.source_sha != run.head_sha
    {
        return Err(internal("qualification_receipt_provenance_mismatch"));
    }
    Ok(())
}

fn resolve_previous(
    api: &QualificationGitHubApi,
    default_branch: &str,
    repository: &str,
    context: &velnor_actions_contract::QualificationDispatch,
    link: &Option<QualificationCacheReceiptLink>,
    phase: QualificationPhase,
    depth: usize,
) -> Result<Option<Value>, OrchestratorError> {
    match (predecessor_phase(phase), link.as_ref()) {
        (None, None) => Ok(None),
        (Some(previous_phase), Some(link)) => resolve_node(
            api,
            default_branch,
            repository,
            context,
            link.run,
            previous_phase,
            Some(link),
            depth + 1,
        )
        .map(Some),
        _ => Err(internal("qualification_receipt_predecessor_shape")),
    }
}

fn metadata_for(
    run: &RunApiRecord,
    producer: &QualificationCacheProducerContext,
) -> Result<QualificationCacheRunMetadata, OrchestratorError> {
    let conclusion = run
        .conclusion
        .clone()
        .ok_or_else(|| internal("qualification_run_conclusion_missing"))?;
    Ok(QualificationCacheRunMetadata {
        repository: run.repository.clone(),
        default_branch: producer.default_branch.clone(),
        git_ref: producer.git_ref.clone(),
        ref_protected: producer.ref_protected,
        workflow_path_ref: run.path_ref.clone(),
        workflow_ref: producer.workflow_ref.clone(),
        workflow_sha: producer.workflow_sha.clone(),
        head_sha: run.head_sha.clone(),
        event: run.event.clone(),
        conclusion,
        run: QualificationRunRef {
            run_id: run.run_id,
            run_attempt: run.run_attempt,
        },
    })
}

fn predecessor_phase(phase: QualificationPhase) -> Option<QualificationPhase> {
    match phase {
        QualificationPhase::Warm => Some(QualificationPhase::Cold),
        QualificationPhase::Third => Some(QualificationPhase::Warm),
        QualificationPhase::UsefulDelta => Some(QualificationPhase::Third),
        QualificationPhase::Cold | QualificationPhase::Control => None,
    }
}
