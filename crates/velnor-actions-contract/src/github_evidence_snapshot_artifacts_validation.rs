// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::valid_bundle_path;
use crate::{
    GitHubEvidenceArtifactCaptureV1, GitHubEvidenceArtifactRefV1, GitHubEvidenceDocumentKindV1,
    GitHubEvidenceFetchSpecV1, GitHubEvidenceRunAttemptV1, GitHubEvidenceSnapshotV1,
    GitHubEvidenceWorkflowSourceRefV1, MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES,
};

struct ExpectedArtifact<'a> {
    source: &'a GitHubEvidenceArtifactRefV1,
    attempt: &'a GitHubEvidenceRunAttemptV1,
    name: String,
}

pub(in crate::github_evidence_snapshot) fn validate_artifacts(
    snapshot: &GitHubEvidenceSnapshotV1,
    spec: &GitHubEvidenceFetchSpecV1,
    paths: &mut BTreeSet<String>,
    total_bytes: &mut u64,
) -> Result<(), String> {
    let workflows = artifact_workflows(spec);
    if workflows.iter().any(|path| {
        !snapshot
            .run_attempts
            .iter()
            .any(|attempt| !attempt.is_predecessor && attempt.workflow_path.as_str() == *path)
    }) {
        return Err("github_evidence_artifact_producer_runs_missing".to_owned());
    }
    let mut remaining = expected_artifacts(snapshot, spec);
    if remaining.len() != snapshot.artifacts.len() {
        return Err("github_evidence_snapshot_artifact_set".to_owned());
    }
    let mut metadata_indexes = BTreeSet::new();
    for artifact in &snapshot.artifacts {
        let Some(index) = remaining.iter().position(|expected| {
            &artifact.source == expected.source
                && artifact.run_id == expected.attempt.run_id
                && artifact.name == expected.name
        }) else {
            return Err("github_evidence_snapshot_artifact_set".to_owned());
        };
        let expected = remaining.remove(index);
        validate_artifact_record(
            artifact,
            &expected,
            snapshot,
            &mut metadata_indexes,
            paths,
            total_bytes,
        )?;
    }
    validate_artifact_metadata_closure(snapshot, &workflows, remaining.is_empty())
}

fn artifact_workflows(spec: &GitHubEvidenceFetchSpecV1) -> BTreeSet<&str> {
    spec.artifact_sources
        .iter()
        .filter_map(|artifact| {
            spec.workflow_sources.iter().find_map(|workflow| {
                matches!(&workflow.source, GitHubEvidenceWorkflowSourceRefV1::TaskSuite { suite_id } if suite_id == &artifact.suite_id)
                    .then_some(workflow.path.as_str())
            })
        })
        .collect()
}

fn expected_artifacts<'a>(
    snapshot: &'a GitHubEvidenceSnapshotV1,
    spec: &'a GitHubEvidenceFetchSpecV1,
) -> Vec<ExpectedArtifact<'a>> {
    spec.artifact_sources
        .iter()
        .flat_map(|source| {
            let workflow_path = spec
                .workflow_sources
                .iter()
                .find(|workflow| matches!(&workflow.source, GitHubEvidenceWorkflowSourceRefV1::TaskSuite { suite_id } if suite_id == &source.suite_id))
                .map(|workflow| workflow.path.as_str());
            snapshot
                .run_attempts
                .iter()
                .filter(move |attempt| {
                    !attempt.is_predecessor
                        && Some(attempt.workflow_path.as_str()) == workflow_path
                })
                .map(move |attempt| ExpectedArtifact {
                    source,
                    attempt,
                    name: crate::config::mise_task_suite_artifact_name_for_attempt(
                        &source.suite_id,
                        &source.artifact_name,
                        attempt.run_id,
                        attempt.attempt,
                    ),
                })
        })
        .collect()
}

fn validate_artifact_record(
    artifact: &GitHubEvidenceArtifactCaptureV1,
    expected: &ExpectedArtifact<'_>,
    snapshot: &GitHubEvidenceSnapshotV1,
    metadata_indexes: &mut BTreeSet<u32>,
    paths: &mut BTreeSet<String>,
    total_bytes: &mut u64,
) -> Result<(), String> {
    let metadata_matches = snapshot
        .documents
        .get(artifact.metadata_document as usize)
        .is_some_and(|document| {
            document.kind == GitHubEvidenceDocumentKindV1::ArtifactById
                && document.workflow_path.as_deref()
                    == Some(expected.attempt.workflow_path.as_str())
                && document.run_id == Some(artifact.run_id)
                && document.attempt == Some(expected.attempt.attempt)
                && document.artifact_id == Some(artifact.artifact_id)
        });
    if !metadata_indexes.insert(artifact.metadata_document)
        || artifact.source != *expected.source
        || artifact.run_id != expected.attempt.run_id
        || artifact.name != expected.name
        || !metadata_matches
        || artifact.artifact_id == 0
        || artifact.size_bytes == 0
        || artifact.size_bytes > MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES
        || !crate::ids::is_lower_hex_len(&artifact.sha256, 64)
        || artifact.path != format!("archives/{}.zip", artifact.artifact_id)
        || !valid_bundle_path(&artifact.path)
        || !paths.insert(artifact.path.clone())
    {
        return Err("github_evidence_snapshot_artifact_identity".to_owned());
    }
    *total_bytes = total_bytes.saturating_add(artifact.size_bytes);
    Ok(())
}

fn validate_artifact_metadata_closure(
    snapshot: &GitHubEvidenceSnapshotV1,
    workflows: &BTreeSet<&str>,
    expected_artifacts_consumed: bool,
) -> Result<(), String> {
    let metadata_count = snapshot
        .documents
        .iter()
        .filter(|document| document.kind == GitHubEvidenceDocumentKindV1::ArtifactById)
        .count();
    if !expected_artifacts_consumed
        || metadata_count != snapshot.artifacts.len()
        || snapshot.run_attempts.iter().any(|attempt| {
            !attempt.is_predecessor
                && workflows.contains(attempt.workflow_path.as_str())
                && !snapshot.documents.iter().any(|document| {
                    document.kind == GitHubEvidenceDocumentKindV1::RunArtifactsPage
                        && document.workflow_path.as_deref() == Some(attempt.workflow_path.as_str())
                        && document.run_id == Some(attempt.run_id)
                        && document.page == Some(1)
                })
        })
    {
        return Err("github_evidence_artifact_metadata_missing".to_owned());
    }
    Ok(())
}
