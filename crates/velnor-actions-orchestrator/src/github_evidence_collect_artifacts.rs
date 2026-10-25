use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    GitHubEvidenceArtifactCaptureV1, GitHubEvidenceArtifactRefV1, GitHubEvidenceDocumentKindV1,
    GitHubEvidenceWorkflowRefV1, GitHubEvidenceWorkflowSourceRefV1,
    MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES, MAX_GITHUB_EVIDENCE_ARCHIVES,
    MAX_GITHUB_EVIDENCE_BUNDLE_BYTES, mise_task_suite_artifact_name_for_attempt,
};

use super::api::{self, ArtifactResponse, ArtifactsPage, EvidenceResult};
use super::{Collector, EvidenceDocumentIdentity, MAX_API_PAGES, PAGE_SIZE};
use crate::api::github_evidence_http::{ArtifactArchiveRequest, EvidenceRoute, GitHubEvidenceRead};
use crate::internal::internal;

impl<R: GitHubEvidenceRead> Collector<'_, R> {
    pub(super) fn collect_artifacts(
        &mut self,
        run_attempts: &[velnor_actions_contract::GitHubEvidenceRunAttemptV1],
    ) -> EvidenceResult<()> {
        let sources = self.invocation.spec.artifact_sources.clone();
        let workflow_paths =
            artifact_workflow_paths(&sources, &self.invocation.spec.workflow_sources)?;
        let runs = producer_runs(run_attempts, &workflow_paths);
        if runs.is_empty() {
            return Err(internal("github_evidence_artifact_producer_runs_missing"));
        }
        let mut inventory = BTreeMap::<(String, u64), Vec<ArtifactResponse>>::new();
        for ((workflow_path, run_id), attempts) in &runs {
            let first = attempts
                .first()
                .ok_or_else(|| internal("github_evidence_artifact_attempt_missing"))?;
            let artifacts = self.list_run_artifacts(&workflow_path.clone(), *run_id, first)?;
            inventory.insert((workflow_path.clone(), *run_id), artifacts);
        }

        self.capture_declared_artifacts(&sources, &workflow_paths, &runs, &inventory)?;
        if self.artifacts.len() > MAX_GITHUB_EVIDENCE_ARCHIVES {
            return Err(internal("github_evidence_archive_count_limit"));
        }
        Ok(())
    }

    fn capture_declared_artifacts(
        &mut self,
        sources: &[GitHubEvidenceArtifactRefV1],
        workflow_paths: &BTreeMap<String, String>,
        runs: &BTreeMap<(String, u64), Vec<&velnor_actions_contract::GitHubEvidenceRunAttemptV1>>,
        inventory: &BTreeMap<(String, u64), Vec<ArtifactResponse>>,
    ) -> EvidenceResult<()> {
        let mut artifact_ids = BTreeSet::new();
        for source in sources {
            let workflow_path = workflow_paths
                .get(&source.suite_id)
                .ok_or_else(|| internal("github_evidence_artifact_workflow_missing"))?;
            let producer_runs = runs
                .iter()
                .filter(|((path, _), _)| path == workflow_path)
                .collect::<Vec<_>>();
            if producer_runs.is_empty() {
                return Err(internal("github_evidence_artifact_producer_runs_missing"));
            }
            for ((_, run_id), attempts) in producer_runs {
                let expected_attempts = attempts
                    .iter()
                    .map(|attempt| attempt.attempt)
                    .collect::<BTreeSet<_>>();
                let attempt_name_prefix = mise_task_suite_artifact_name_for_attempt(
                    &source.suite_id,
                    &source.artifact_name,
                    *run_id,
                    1,
                )
                .strip_suffix('1')
                .ok_or_else(|| internal("github_evidence_artifact_name"))?
                .to_owned();
                let listed = inventory
                    .get(&(workflow_path.clone(), *run_id))
                    .ok_or_else(|| internal("github_evidence_artifact_list_missing"))?;
                if listed.iter().any(|artifact| {
                    artifact
                        .name
                        .strip_prefix(&attempt_name_prefix)
                        .is_some_and(|suffix| {
                            suffix.parse::<u32>().ok().is_none_or(|attempt| {
                                !expected_attempts.contains(&attempt)
                                    || suffix != attempt.to_string()
                            })
                        })
                }) {
                    return Err(internal("github_evidence_artifact_unexpected_attempt"));
                }
                for attempt in attempts {
                    let expected_name = mise_task_suite_artifact_name_for_attempt(
                        &source.suite_id,
                        &source.artifact_name,
                        attempt.run_id,
                        attempt.attempt,
                    );
                    let matching = listed
                        .iter()
                        .filter(|artifact| artifact.name == expected_name)
                        .collect::<Vec<_>>();
                    if matching.len() != 1 {
                        return Err(internal("github_evidence_artifact_missing_or_ambiguous"));
                    }
                    let selected = matching[0];
                    let sha256 = api::artifact_digest(selected.digest.as_deref())
                        .ok_or_else(|| internal("github_evidence_artifact_digest_missing"))?;
                    self.validate_artifact(selected, attempt, &expected_name, &sha256)?;
                    if !artifact_ids.insert(selected.id) {
                        return Err(internal("github_evidence_artifact_duplicate_id"));
                    }
                    self.capture_artifact(source.clone(), attempt, selected, &sha256)?;
                }
            }
        }
        Ok(())
    }

    fn list_run_artifacts(
        &mut self,
        workflow_path: &str,
        run_id: u64,
        attempt: &velnor_actions_contract::GitHubEvidenceRunAttemptV1,
    ) -> EvidenceResult<Vec<ArtifactResponse>> {
        let mut all = Vec::new();
        let mut ids = BTreeSet::new();
        let mut expected_total = None;
        let mut page = 1;
        loop {
            if page > MAX_API_PAGES {
                return Err(internal("github_evidence_artifact_pages_limit"));
            }
            let (body, response): (Vec<u8>, ArtifactsPage) =
                self.read_json(&EvidenceRoute::RunArtifacts { run_id, page })?;
            if response.total_count > MAX_GITHUB_EVIDENCE_ARCHIVES as u64
                || expected_total
                    .replace(response.total_count)
                    .is_some_and(|old| old != response.total_count)
                || api::expected_page_items(response.total_count, PAGE_SIZE, page)?
                    != response.artifacts.len()
            {
                return Err(internal("github_evidence_artifact_pages_incomplete"));
            }
            for artifact in &response.artifacts {
                if artifact.id == 0 || !ids.insert(artifact.id) {
                    return Err(internal("github_evidence_artifact_list_identity"));
                }
            }
            self.capture_document(
                EvidenceDocumentIdentity {
                    kind: GitHubEvidenceDocumentKindV1::RunArtifactsPage,
                    workflow_path: Some(workflow_path.to_owned()),
                    run_id: Some(run_id),
                    attempt: None,
                    artifact_id: None,
                    page: Some(page),
                    path: format!(
                        "documents/artifacts-{}-{run_id}-{page:04}.json",
                        workflow_slug(workflow_path)?
                    ),
                },
                &body,
            )?;
            all.extend(response.artifacts);
            let total = expected_total.unwrap_or_default();
            if page == api::expected_pages(total, PAGE_SIZE)? {
                break;
            }
            page += 1;
        }
        if all.iter().any(|artifact| {
            artifact.workflow_run.as_ref().is_some_and(|source| {
                source.id != attempt.run_id
                    || source.repository_id != self.invocation.repository_id
                    || source.head_repository_id != self.invocation.repository_id
                    || source.head_sha != attempt.head_sha
                    || source.head_branch != attempt.head_branch
            })
        }) {
            return Err(internal("github_evidence_artifact_run_lineage"));
        }
        Ok(all)
    }

    fn validate_artifact(
        &self,
        artifact: &ArtifactResponse,
        attempt: &velnor_actions_contract::GitHubEvidenceRunAttemptV1,
        expected_name: &str,
        expected_digest: &str,
    ) -> EvidenceResult<()> {
        let source = artifact
            .workflow_run
            .as_ref()
            .ok_or_else(|| internal("github_evidence_artifact_lineage_missing"))?;
        if artifact.id == 0
            || artifact.name != expected_name
            || artifact.expired
            || artifact.size_in_bytes == 0
            || artifact.size_in_bytes > MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES
            || source.id != attempt.run_id
            || source.repository_id != self.invocation.repository_id
            || source.head_repository_id != self.invocation.repository_id
            || source.head_sha != attempt.head_sha
            || source.head_branch != attempt.head_branch
            || api::artifact_digest(artifact.digest.as_deref()).as_deref() != Some(expected_digest)
        {
            return Err(internal("github_evidence_artifact_identity"));
        }
        Ok(())
    }

    fn capture_artifact(
        &mut self,
        source: GitHubEvidenceArtifactRefV1,
        attempt: &velnor_actions_contract::GitHubEvidenceRunAttemptV1,
        selected: &ArtifactResponse,
        expected_sha256: &str,
    ) -> EvidenceResult<()> {
        if self.artifacts.len() >= MAX_GITHUB_EVIDENCE_ARCHIVES
            || self.total_bytes.saturating_add(selected.size_in_bytes)
                > MAX_GITHUB_EVIDENCE_BUNDLE_BYTES
        {
            return Err(internal("github_evidence_archive_bound"));
        }
        let route = EvidenceRoute::ArtifactById {
            artifact_id: selected.id,
        };
        let (body, by_id): (Vec<u8>, ArtifactResponse) = self.read_json(&route)?;
        if &by_id != selected
            || by_id.expired
            || api::artifact_digest(by_id.digest.as_deref()).as_deref() != Some(expected_sha256)
        {
            return Err(internal("github_evidence_artifact_by_id_mismatch"));
        }
        let metadata_document = self.capture_document(
            EvidenceDocumentIdentity {
                kind: GitHubEvidenceDocumentKindV1::ArtifactById,
                workflow_path: Some(source.workflow_path.clone()),
                run_id: Some(attempt.run_id),
                attempt: Some(attempt.attempt),
                artifact_id: Some(selected.id),
                page: None,
                path: format!("documents/artifact-{}.json", selected.id),
            },
            &body,
        )?;
        let path = format!("archives/{}.zip", selected.id);
        let destination = self.bundle.path_for(&path)?;
        let (size, digest) =
            self.download_and_verify_archive(selected, expected_sha256, &destination)?;
        let zip_deadline = velnor_actions_mise::CheckDeadline::from_start(
            self.deadline,
            std::time::Duration::ZERO,
        )
        .map_err(|_| internal("github_evidence_archive_deadline"))?;
        let mut archive = std::fs::File::open(&destination)
            .map_err(|_| internal("github_evidence_archive_read"))?;
        let Ok(unpacked) = crate::package_release_publish::artifact_zip::validate_evidence_archive(
            &mut archive,
            zip_deadline,
        ) else {
            drop(std::fs::remove_file(&destination));
            return Err(internal("github_evidence_archive_invalid"));
        };
        let Some(total_unpacked) = self
            .total_unpacked_bytes
            .checked_add(unpacked)
            .filter(|total| *total <= MAX_GITHUB_EVIDENCE_BUNDLE_BYTES)
        else {
            drop(std::fs::remove_file(&destination));
            return Err(internal("github_evidence_archive_unpacked_total"));
        };
        self.total_unpacked_bytes = total_unpacked;
        self.total_bytes = self.total_bytes.saturating_add(size);
        if self.total_bytes > MAX_GITHUB_EVIDENCE_BUNDLE_BYTES {
            return Err(internal("github_evidence_bundle_too_large"));
        }
        self.artifacts.push(GitHubEvidenceArtifactCaptureV1 {
            source,
            artifact_id: selected.id,
            run_id: attempt.run_id,
            name: selected.name.clone(),
            path,
            metadata_document,
            size_bytes: size,
            sha256: digest,
        });
        Ok(())
    }

    fn download_and_verify_archive(
        &self,
        selected: &ArtifactResponse,
        expected_sha256: &str,
        destination: &std::path::Path,
    ) -> EvidenceResult<(u64, String)> {
        let receipt = self.reader.archive(ArtifactArchiveRequest {
            repo: &self.invocation.repository,
            artifact_id: selected.id,
            token: &self.invocation.token,
            expected_size: selected.size_in_bytes,
            expected_sha256,
            dest: destination,
            deadline: self.deadline,
        })?;
        let (size, digest) = super::bundle::sha256_file(destination)?;
        if receipt.size_bytes != selected.size_in_bytes
            || receipt.sha256 != expected_sha256
            || size != selected.size_in_bytes
            || digest != expected_sha256
        {
            drop(std::fs::remove_file(destination));
            return Err(internal("github_evidence_archive_receipt_mismatch"));
        }
        Ok((size, digest))
    }
}

fn artifact_workflow_paths(
    sources: &[GitHubEvidenceArtifactRefV1],
    workflows: &[GitHubEvidenceWorkflowRefV1],
) -> EvidenceResult<BTreeMap<String, String>> {
    let mut paths = BTreeMap::new();
    for artifact in sources {
        let path = workflows
            .iter()
            .find_map(|workflow| match &workflow.source {
                GitHubEvidenceWorkflowSourceRefV1::TaskSuite { suite_id }
                    if suite_id == &artifact.suite_id =>
                {
                    Some(workflow.path.clone())
                }
                _ => None,
            })
            .ok_or_else(|| internal("github_evidence_artifact_workflow_missing"))?;
        paths.insert(artifact.suite_id.clone(), path);
    }
    Ok(paths)
}

fn producer_runs<'a>(
    run_attempts: &'a [velnor_actions_contract::GitHubEvidenceRunAttemptV1],
    workflow_paths: &BTreeMap<String, String>,
) -> BTreeMap<(String, u64), Vec<&'a velnor_actions_contract::GitHubEvidenceRunAttemptV1>> {
    let mut runs = BTreeMap::<
        (String, u64),
        Vec<&'a velnor_actions_contract::GitHubEvidenceRunAttemptV1>,
    >::new();
    for attempt in run_attempts {
        if !attempt.is_predecessor
            && workflow_paths
                .values()
                .any(|path| path == &attempt.workflow_path)
        {
            runs.entry((attempt.workflow_path.clone(), attempt.run_id))
                .or_default()
                .push(attempt);
        }
    }
    runs
}

fn workflow_slug(path: &str) -> EvidenceResult<&str> {
    path.strip_prefix(".github/workflows/")
        .and_then(|value| value.strip_suffix(".yml"))
        .ok_or_else(|| internal("github_evidence_workflow_path"))
}
