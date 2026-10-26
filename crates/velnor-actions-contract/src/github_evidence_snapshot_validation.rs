// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "github_evidence_snapshot_temporal.rs"]
mod temporal;

#[path = "github_evidence_snapshot_artifacts_validation.rs"]
mod artifacts;

pub(super) use artifacts::validate_artifacts;

#[must_use]
pub fn is_valid_github_evidence_status(value: &str) -> bool {
    matches!(
        value,
        "completed" | "in_progress" | "queued" | "requested" | "waiting" | "pending"
    )
}

#[must_use]
pub fn is_valid_github_evidence_conclusion(value: Option<&str>) -> bool {
    value.is_none_or(|value| {
        matches!(
            value,
            "success"
                | "failure"
                | "neutral"
                | "cancelled"
                | "skipped"
                | "timed_out"
                | "action_required"
                | "stale"
                | "startup_failure"
        )
    })
}

use std::collections::{BTreeMap, BTreeSet};

use super::{
    GitHubEvidenceAcquisitionV1, GitHubEvidenceDocumentKindV1, GitHubEvidenceDocumentV1,
    GitHubEvidenceFetchSpecV1, GitHubEvidenceRunAttemptV1, GitHubEvidenceSnapshotV1,
};

type PageIdentity = (
    GitHubEvidenceDocumentKindV1,
    String,
    Option<u64>,
    Option<u32>,
);
type AttemptIdentity<'a> = (&'a str, u64, u32);
type WorkflowRun<'a> = (&'a str, u64);
type PredecessorRuns<'a> = BTreeMap<&'a str, BTreeSet<u64>>;
type RunAttempts<'a> = BTreeMap<WorkflowRun<'a>, Vec<u32>>;

pub(super) fn is_utc_timestamp(value: &str) -> bool {
    temporal::is_utc_timestamp(value)
}

pub(super) fn valid_bundle_path(path: &str) -> bool {
    temporal::valid_bundle_path(path)
}

pub(super) fn valid_window(acquisition: &GitHubEvidenceAcquisitionV1, days: u8) -> bool {
    temporal::valid_window(acquisition, days)
}

pub(super) fn valid_document_identity(document: &GitHubEvidenceDocumentV1) -> bool {
    let scoped_workflow = document.workflow_path.as_deref().is_some_and(|path| {
        path.starts_with(".github/workflows/")
            && path
                .rsplit_once('.')
                .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("yml"))
    });
    match document.kind {
        GitHubEvidenceDocumentKindV1::WorkflowInventory => {
            document.workflow_path.is_none()
                && document.run_id.is_none()
                && document.attempt.is_none()
                && document.artifact_id.is_none()
                && document.page.is_some_and(|page| page > 0)
        }
        GitHubEvidenceDocumentKindV1::Repository => {
            document.workflow_path.is_none()
                && document.run_id.is_none()
                && document.attempt.is_none()
                && document.artifact_id.is_none()
                && document.page.is_none()
        }
        GitHubEvidenceDocumentKindV1::WorkflowRunsPage => {
            scoped_workflow
                && document.run_id.is_none()
                && document.attempt.is_none()
                && document.artifact_id.is_none()
                && document.page.is_some_and(|page| page > 0)
        }
        GitHubEvidenceDocumentKindV1::WorkflowPredecessor => {
            scoped_workflow
                && document.run_id.is_none()
                && document.attempt.is_none()
                && document.artifact_id.is_none()
                && document.page == Some(1)
        }
        GitHubEvidenceDocumentKindV1::RunAttempt => {
            scoped_workflow
                && document.run_id.is_some_and(|id| id > 0)
                && document.attempt.is_some_and(|attempt| attempt > 0)
                && document.artifact_id.is_none()
                && document.page.is_none()
        }
        GitHubEvidenceDocumentKindV1::RunAttemptJobsPage => {
            scoped_workflow
                && document.run_id.is_some_and(|id| id > 0)
                && document.attempt.is_some_and(|attempt| attempt > 0)
                && document.artifact_id.is_none()
                && document.page.is_some_and(|page| page > 0)
        }
        GitHubEvidenceDocumentKindV1::RunArtifactsPage => {
            scoped_workflow
                && document.run_id.is_some_and(|id| id > 0)
                && document.attempt.is_none()
                && document.artifact_id.is_none()
                && document.page.is_some_and(|page| page > 0)
        }
        GitHubEvidenceDocumentKindV1::ArtifactById => {
            scoped_workflow
                && document.run_id.is_some_and(|id| id > 0)
                && document.attempt.is_some_and(|attempt| attempt > 0)
                && document.artifact_id.is_some_and(|id| id > 0)
                && document.page.is_none()
        }
    }
}

pub(super) fn validate_document_pages(
    documents: &[GitHubEvidenceDocumentV1],
    spec: &GitHubEvidenceFetchSpecV1,
) -> Result<(), String> {
    let mut page_sets = BTreeMap::<PageIdentity, Vec<u32>>::new();
    let mut attempt_docs = BTreeSet::new();
    let mut repository_count = 0;
    let expected_workflows = spec
        .workflow_sources
        .iter()
        .map(|source| source.path.as_str())
        .collect::<BTreeSet<_>>();
    for document in documents {
        if document
            .workflow_path
            .as_deref()
            .is_some_and(|path| !expected_workflows.contains(path))
        {
            return Err("github_evidence_unrequested_workflow_document".to_owned());
        }
        if document.kind == GitHubEvidenceDocumentKindV1::Repository {
            repository_count += 1;
            continue;
        }
        let workflow = document.workflow_path.clone().unwrap_or_default();
        match document.kind {
            GitHubEvidenceDocumentKindV1::RunAttempt => {
                if !attempt_docs.insert((
                    workflow,
                    document.run_id.unwrap_or_default(),
                    document.attempt.unwrap_or_default(),
                )) {
                    return Err("github_evidence_duplicate_attempt_document".to_owned());
                }
                continue;
            }
            GitHubEvidenceDocumentKindV1::ArtifactById => continue,
            _ => {}
        }
        let key = (document.kind, workflow, document.run_id, document.attempt);
        page_sets
            .entry(key)
            .or_default()
            .push(document.page.unwrap_or_default());
    }
    if repository_count != 1 {
        return Err("github_evidence_repository_document_count".to_owned());
    }
    if !documents.iter().any(|document| {
        document.kind == GitHubEvidenceDocumentKindV1::WorkflowInventory && document.page == Some(1)
    }) {
        return Err("github_evidence_workflow_inventory_missing".to_owned());
    }
    for pages in page_sets.values_mut() {
        pages.sort_unstable();
        if pages.first() != Some(&1) || pages.windows(2).any(|pair| pair[1] != pair[0] + 1) {
            return Err("github_evidence_document_pages_incomplete".to_owned());
        }
    }
    Ok(())
}

pub(super) fn validate_attempts(
    snapshot: &GitHubEvidenceSnapshotV1,
    spec: &GitHubEvidenceFetchSpecV1,
) -> Result<(), String> {
    let workflow_paths = spec
        .workflow_sources
        .iter()
        .map(|source| source.path.as_str())
        .collect::<BTreeSet<_>>();
    validate_attempt_page_coverage(snapshot, &workflow_paths)?;
    let (predecessors, mut attempts) = collect_attempts(snapshot, spec, &workflow_paths)?;
    validate_predecessor_coverage(&workflow_paths, &predecessors)?;
    validate_attempt_sequences(&mut attempts)
}

fn validate_attempt_page_coverage(
    snapshot: &GitHubEvidenceSnapshotV1,
    workflow_paths: &BTreeSet<&str>,
) -> Result<(), String> {
    let missing = workflow_paths.iter().any(|path| {
        !snapshot.documents.iter().any(|document| {
            document.kind == GitHubEvidenceDocumentKindV1::WorkflowRunsPage
                && document.workflow_path.as_deref() == Some(path)
                && document.page == Some(1)
        }) || !snapshot.documents.iter().any(|document| {
            document.kind == GitHubEvidenceDocumentKindV1::WorkflowPredecessor
                && document.workflow_path.as_deref() == Some(path)
                && document.page == Some(1)
        })
    });
    if missing {
        return Err("github_evidence_workflow_run_window_or_predecessor_missing".to_owned());
    }
    Ok(())
}

fn collect_attempts<'a>(
    snapshot: &'a GitHubEvidenceSnapshotV1,
    spec: &GitHubEvidenceFetchSpecV1,
    workflow_paths: &BTreeSet<&str>,
) -> Result<(PredecessorRuns<'a>, RunAttempts<'a>), String> {
    let mut seen = BTreeSet::new();
    let mut predecessors = PredecessorRuns::new();
    let mut attempts = RunAttempts::new();
    let mut prior = None;
    for attempt in &snapshot.run_attempts {
        let identity = validate_attempt_shape(
            attempt,
            workflow_paths,
            &spec.default_branch,
            &mut seen,
            prior,
        )?;
        prior = Some(identity);
        validate_attempt_documents(snapshot, attempt)?;
        record_attempt_time(
            attempt,
            &snapshot.acquisition,
            &mut predecessors,
            &mut attempts,
        )?;
    }
    Ok((predecessors, attempts))
}

fn validate_attempt_shape<'a>(
    attempt: &'a GitHubEvidenceRunAttemptV1,
    workflow_paths: &BTreeSet<&str>,
    default_branch: &str,
    seen: &mut BTreeSet<AttemptIdentity<'a>>,
    prior: Option<AttemptIdentity<'a>>,
) -> Result<AttemptIdentity<'a>, String> {
    let identity = (
        attempt.workflow_path.as_str(),
        attempt.run_id,
        attempt.attempt,
    );
    if !workflow_paths.contains(attempt.workflow_path.as_str())
        || attempt.run_id == 0
        || attempt.attempt == 0
        || !is_utc_timestamp(&attempt.created_at)
        || attempt.event != "push"
        || attempt.head_branch != default_branch
        || !crate::ids::is_lower_hex_len(&attempt.head_sha, 40)
        || !is_valid_github_evidence_status(&attempt.status)
        || !is_valid_github_evidence_conclusion(attempt.conclusion.as_deref())
        || !seen.insert(identity)
        || prior.is_some_and(|value| value >= identity)
        || attempt.jobs_documents.is_empty()
        || !attempt
            .jobs_documents
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    {
        return Err("github_evidence_snapshot_attempt".to_owned());
    }
    Ok(identity)
}

fn validate_attempt_documents(
    snapshot: &GitHubEvidenceSnapshotV1,
    attempt: &GitHubEvidenceRunAttemptV1,
) -> Result<(), String> {
    let detail_count = snapshot
        .documents
        .iter()
        .filter(|document| {
            document.kind == GitHubEvidenceDocumentKindV1::RunAttempt
                && document.workflow_path.as_deref() == Some(attempt.workflow_path.as_str())
                && document.run_id == Some(attempt.run_id)
                && document.attempt == Some(attempt.attempt)
        })
        .count();
    let expected_jobs = snapshot
        .documents
        .iter()
        .enumerate()
        .filter(|(_, document)| {
            document.kind == GitHubEvidenceDocumentKindV1::RunAttemptJobsPage
                && document.workflow_path.as_deref() == Some(attempt.workflow_path.as_str())
                && document.run_id == Some(attempt.run_id)
                && document.attempt == Some(attempt.attempt)
        })
        .map(|(index, _)| u32::try_from(index))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "github_evidence_snapshot_jobs_reference".to_owned())?;
    if detail_count != 1 || expected_jobs != attempt.jobs_documents {
        return Err("github_evidence_snapshot_jobs_identity".to_owned());
    }
    Ok(())
}

fn record_attempt_time<'a>(
    attempt: &'a GitHubEvidenceRunAttemptV1,
    acquisition: &GitHubEvidenceAcquisitionV1,
    predecessors: &mut PredecessorRuns<'a>,
    attempts: &mut RunAttempts<'a>,
) -> Result<(), String> {
    if attempt.is_predecessor {
        if attempt.created_at >= acquisition.window_start {
            return Err("github_evidence_predecessor_inside_window".to_owned());
        }
        predecessors
            .entry(&attempt.workflow_path)
            .or_default()
            .insert(attempt.run_id);
    } else if attempt.created_at < acquisition.window_start
        || attempt.created_at >= acquisition.window_end
    {
        return Err("github_evidence_attempt_outside_window".to_owned());
    }
    attempts
        .entry((&attempt.workflow_path, attempt.run_id))
        .or_default()
        .push(attempt.attempt);
    Ok(())
}

fn validate_predecessor_coverage(
    workflow_paths: &BTreeSet<&str>,
    predecessors: &PredecessorRuns<'_>,
) -> Result<(), String> {
    if workflow_paths
        .iter()
        .any(|path| predecessors.get(path).is_none_or(|runs| runs.len() != 1))
    {
        return Err("github_evidence_predecessor_missing_or_ambiguous".to_owned());
    }
    Ok(())
}

fn validate_attempt_sequences(attempts: &mut RunAttempts<'_>) -> Result<(), String> {
    let incomplete = attempts.values_mut().any(|numbers| {
        numbers.sort_unstable();
        numbers.first() != Some(&1) || numbers.windows(2).any(|pair| pair[1] != pair[0] + 1)
    });
    if incomplete {
        return Err("github_evidence_attempt_history_incomplete".to_owned());
    }
    Ok(())
}
