// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The receipt walk: for a run, its metadata, its one receipt artifact (listed, then re-read by
//! id), the downloaded archive and the receipt inside it, then the same for the run the receipt
//! names as its predecessor, to a depth of three.

use serde::Deserialize;

use super::api::{Client, required};
use super::archive::receipt_bytes;
use super::types::{
    Artifact, MAX_RECEIPT_BYTES, Node, RECEIPT_ARTIFACT, Receipt, ReceiptDocument, RunMetadata,
    RunRef,
};
use crate::error::CiError;
use crate::write_request::internal;

const MAX_LINEAGE_NODES: usize = 3;

#[derive(Deserialize)]
struct ArtifactList {
    total_count: Option<u64>,
    artifacts: Option<Vec<ArtifactResponse>>,
}

#[derive(Deserialize)]
struct ArtifactResponse {
    id: Option<u64>,
    name: Option<String>,
    digest: Option<String>,
    size_in_bytes: Option<u64>,
    expired: Option<bool>,
    workflow_run: Option<ArtifactRun>,
}

#[derive(Deserialize)]
struct ArtifactRun {
    id: Option<u64>,
    head_branch: Option<String>,
    head_sha: Option<String>,
}

pub(super) fn resolve(
    client: &Client<'_>,
    run: RunRef,
    depth: usize,
    expected: Option<&(u64, String)>,
) -> Result<Node, CiError> {
    if depth == 0 || depth > MAX_LINEAGE_NODES {
        return Err(internal("qualification_lineage_depth"));
    }
    let metadata = client.run_metadata(run)?;
    let artifact = select_artifact(client, run)?;
    if expected.is_some_and(|(id, digest)| id != &artifact.id || digest != &artifact.digest) {
        return Err(internal("qualification_artifact_link_mismatch"));
    }
    verify_artifact_run(&artifact, &metadata)?;
    let document = download_document(client, &artifact)?;
    if document.schema != 2 || document.producer.run != run || document.receipt.run != run {
        return Err(internal("qualification_receipt_run_mismatch"));
    }
    let previous = load_predecessor(client, &document.receipt, depth)?;
    Ok(Node {
        metadata,
        artifact,
        producer: document.producer,
        receipt: document.receipt,
        previous,
    })
}

fn select_artifact(client: &Client<'_>, run: RunRef) -> Result<Artifact, CiError> {
    let route = format!(
        "repos/{}/actions/runs/{}/artifacts?per_page=100",
        client.repository(),
        run.run_id
    );
    let list: ArtifactList = client.json(&route)?;
    let entries = list
        .artifacts
        .ok_or_else(|| internal("qualification_artifacts"))?;
    if list.total_count != Some(entries.len() as u64) {
        return Err(internal("qualification_artifact_list_incomplete"));
    }
    let mut matches = entries
        .into_iter()
        .filter(|entry| entry.name.as_deref() == Some(RECEIPT_ARTIFACT));
    let selected = matches
        .next()
        .ok_or_else(|| internal("qualification_artifact_missing"))?;
    if matches.next().is_some() {
        return Err(internal("qualification_artifact_ambiguous"));
    }
    let selected = materialize(selected)?;
    let route = format!(
        "repos/{}/actions/artifacts/{}",
        client.repository(),
        selected.id
    );
    let confirmed = materialize(client.json(&route)?)?;
    if confirmed != selected {
        return Err(internal("qualification_artifact_metadata_changed"));
    }
    Ok(selected)
}

fn materialize(raw: ArtifactResponse) -> Result<Artifact, CiError> {
    let run = raw
        .workflow_run
        .ok_or_else(|| internal("qualification_artifact_run_missing"))?;
    let artifact = Artifact {
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
        workflow_run_id: run
            .id
            .ok_or_else(|| internal("qualification_artifact_run_id"))?,
        workflow_head_branch: required(run.head_branch, "qualification_artifact_head_branch")?,
        workflow_head_sha: required(run.head_sha, "qualification_artifact_head_sha")?,
    };
    if artifact.id == 0
        || artifact.expired
        || artifact.size_bytes == 0
        || artifact.size_bytes > MAX_RECEIPT_BYTES as u64
    {
        return Err(internal("qualification_artifact_invalid"));
    }
    Ok(artifact)
}

fn verify_artifact_run(artifact: &Artifact, metadata: &RunMetadata) -> Result<(), CiError> {
    if artifact.name != RECEIPT_ARTIFACT
        || artifact.workflow_run_id != metadata.run.run_id
        || artifact.workflow_head_branch != metadata.default_branch
        || artifact.workflow_head_sha != metadata.head_sha
    {
        return Err(internal("qualification_artifact_run_mismatch"));
    }
    Ok(())
}

fn download_document(client: &Client<'_>, artifact: &Artifact) -> Result<ReceiptDocument, CiError> {
    let route = format!(
        "repos/{}/actions/artifacts/{}/zip",
        client.repository(),
        artifact.id
    );
    let archive = client.bytes(&route)?;
    let receipt = receipt_bytes(&archive, artifact)?;
    if receipt.len() > MAX_RECEIPT_BYTES {
        return Err(internal("qualification_receipt_oversize"));
    }
    serde_json::from_slice(&receipt).map_err(|_| internal("qualification_receipt_document"))
}

fn load_predecessor(
    client: &Client<'_>,
    receipt: &Receipt,
    depth: usize,
) -> Result<Option<Box<Node>>, CiError> {
    let Some(link) = receipt.predecessor.as_ref() else {
        return Ok(None);
    };
    if depth >= MAX_LINEAGE_NODES {
        return Err(internal("qualification_lineage_depth"));
    }
    let previous = resolve(
        client,
        link.run,
        depth + 1,
        Some(&(link.artifact_id, link.artifact_digest.clone())),
    )?;
    Ok(Some(Box::new(previous)))
}
