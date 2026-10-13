//! Immutable Actions artifact selection, download, and lineage assembly.

use serde::Deserialize;
use velnor_actions_contract::{
    MAX_QUALIFICATION_RECEIPT_BYTES, QualificationCacheArtifact, QualificationCacheProducerContext,
    QualificationCacheReceipt, QualificationCacheReceiptArtifactDocument,
    QualificationCacheRunMetadata, QualificationRunRef,
};

use crate::OrchestratorError;
use crate::internal::internal;

use super::client::GitHub;
use super::run::load_run_metadata;

const MAX_LINEAGE_NODES: usize = 3;

#[derive(Debug, serde::Serialize)]
pub(in crate::qualification_resolver) struct FetchedNode {
    pub(in crate::qualification_resolver) metadata: QualificationCacheRunMetadata,
    pub(in crate::qualification_resolver) artifact: QualificationCacheArtifact,
    pub(in crate::qualification_resolver) producer: QualificationCacheProducerContext,
    pub(in crate::qualification_resolver) receipt: QualificationCacheReceipt,
    pub(in crate::qualification_resolver) previous: Option<Box<Self>>,
}

#[derive(Debug, Deserialize)]
struct ArtifactList {
    total_count: Option<u64>,
    artifacts: Option<Vec<ArtifactResponse>>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ArtifactResponse {
    id: Option<u64>,
    name: Option<String>,
    digest: Option<String>,
    size_in_bytes: Option<u64>,
    expired: Option<bool>,
    workflow_run: Option<ArtifactRun>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ArtifactRun {
    id: Option<u64>,
    head_branch: Option<String>,
    head_sha: Option<String>,
}

pub(in crate::qualification_resolver) fn resolve_chain(
    client: &GitHub<'_>,
    run: QualificationRunRef,
    depth: usize,
    expected_artifact: Option<&(u64, String)>,
) -> Result<FetchedNode, OrchestratorError> {
    if depth == 0 || depth > MAX_LINEAGE_NODES {
        return Err(internal("qualification_lineage_depth"));
    }
    let metadata = load_run_metadata(client, run)?;
    let artifact = select_artifact(client, run)?;
    verify_expected_artifact(&artifact, expected_artifact)?;
    verify_artifact_run(&artifact, &metadata)?;
    let document = download_document(client, &artifact)?;
    validate_document(&document, run)?;
    let previous = load_predecessor(client, &document.receipt, depth)?;
    Ok(FetchedNode {
        metadata,
        artifact,
        producer: document.producer,
        receipt: document.receipt,
        previous,
    })
}

fn select_artifact(
    client: &GitHub<'_>,
    run: QualificationRunRef,
) -> Result<QualificationCacheArtifact, OrchestratorError> {
    let route = format!(
        "repos/{}/actions/runs/{}/artifacts?per_page=100",
        client.repository(),
        run.run_id
    );
    let list: ArtifactList = GitHub::json(client.catalog(), &route)?;
    let entries = list
        .artifacts
        .ok_or_else(|| internal("qualification_artifacts"))?;
    if list.total_count != Some(entries.len() as u64) {
        return Err(internal("qualification_artifact_list_incomplete"));
    }
    let mut matches = entries.into_iter().filter(|entry| {
        entry.name.as_deref() == Some(velnor_actions_contract::QUALIFICATION_CACHE_RECEIPT_ARTIFACT)
    });
    let selected = matches
        .next()
        .ok_or_else(|| internal("qualification_artifact_missing"))?;
    if matches.next().is_some() {
        return Err(internal("qualification_artifact_ambiguous"));
    }
    let selected = materialize_artifact(selected)?;
    let confirmed = query_artifact_id(client, selected.id)?;
    if confirmed != selected {
        return Err(internal("qualification_artifact_metadata_changed"));
    }
    Ok(selected)
}

fn materialize_artifact(
    raw: ArtifactResponse,
) -> Result<QualificationCacheArtifact, OrchestratorError> {
    let workflow_run = raw
        .workflow_run
        .ok_or_else(|| internal("qualification_artifact_run_missing"))?;
    let artifact = QualificationCacheArtifact {
        id: raw
            .id
            .ok_or_else(|| internal("qualification_artifact_id"))?,
        name: required(raw.name, "qualification_artifact_name")?,
        digest: required(raw.digest, "qualification_artifact_digest")?,
        size_bytes: raw
            .size_in_bytes
            .ok_or_else(|| internal("qualification_artifact_size"))?,
        expired: raw
            .expired
            .ok_or_else(|| internal("qualification_artifact_expiry"))?,
        workflow_run_id: workflow_run
            .id
            .ok_or_else(|| internal("qualification_artifact_run_id"))?,
        workflow_head_branch: required(
            workflow_run.head_branch,
            "qualification_artifact_head_branch",
        )?,
        workflow_head_sha: required(workflow_run.head_sha, "qualification_artifact_head_sha")?,
    };
    if artifact.id == 0
        || artifact.expired
        || artifact.size_bytes == 0
        || artifact.size_bytes > MAX_QUALIFICATION_RECEIPT_BYTES as u64
    {
        return Err(internal("qualification_artifact_invalid"));
    }
    Ok(artifact)
}

fn query_artifact_id(
    client: &GitHub<'_>,
    id: u64,
) -> Result<QualificationCacheArtifact, OrchestratorError> {
    let route = format!("repos/{}/actions/artifacts/{id}", client.repository());
    materialize_artifact(GitHub::json(client.catalog(), &route)?)
}

fn verify_expected_artifact(
    artifact: &QualificationCacheArtifact,
    expected: Option<&(u64, String)>,
) -> Result<(), OrchestratorError> {
    if expected.is_some_and(|(id, digest)| id != &artifact.id || digest != &artifact.digest) {
        return Err(internal("qualification_artifact_link_mismatch"));
    }
    Ok(())
}

fn verify_artifact_run(
    artifact: &QualificationCacheArtifact,
    metadata: &QualificationCacheRunMetadata,
) -> Result<(), OrchestratorError> {
    if artifact.name != velnor_actions_contract::QUALIFICATION_CACHE_RECEIPT_ARTIFACT
        || artifact.workflow_run_id != metadata.run.run_id
        || artifact.workflow_head_branch != metadata.default_branch
        || artifact.workflow_head_sha != metadata.head_sha
    {
        return Err(internal("qualification_artifact_run_mismatch"));
    }
    Ok(())
}

fn download_document(
    client: &GitHub<'_>,
    artifact: &QualificationCacheArtifact,
) -> Result<QualificationCacheReceiptArtifactDocument, OrchestratorError> {
    let route = format!(
        "repos/{}/actions/artifacts/{}/zip",
        client.repository(),
        artifact.id
    );
    let bytes = GitHub::api_bytes(client.catalog(), &route)?;
    let receipt = crate::qualification_resolver::archive::receipt_bytes(&bytes, artifact)?;
    if receipt.len() > MAX_QUALIFICATION_RECEIPT_BYTES {
        return Err(internal("qualification_receipt_oversize"));
    }
    serde_json::from_slice(&receipt).map_err(|_| internal("qualification_receipt_document"))
}

fn validate_document(
    document: &QualificationCacheReceiptArtifactDocument,
    run: QualificationRunRef,
) -> Result<(), OrchestratorError> {
    if document.schema != 1 || document.producer.run != run || document.receipt.run != run {
        return Err(internal("qualification_receipt_run_mismatch"));
    }
    Ok(())
}

fn load_predecessor(
    client: &GitHub<'_>,
    receipt: &QualificationCacheReceipt,
    depth: usize,
) -> Result<Option<Box<FetchedNode>>, OrchestratorError> {
    let Some(link) = receipt.predecessor.as_ref() else {
        return Ok(None);
    };
    if depth >= MAX_LINEAGE_NODES {
        return Err(internal("qualification_lineage_depth"));
    }
    let previous = resolve_chain(
        client,
        link.run,
        depth + 1,
        Some(&(link.artifact_id, link.artifact_digest.clone())),
    )?;
    Ok(Some(Box::new(previous)))
}

fn required(value: Option<String>, error: &'static str) -> Result<String, OrchestratorError> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(error))
}
