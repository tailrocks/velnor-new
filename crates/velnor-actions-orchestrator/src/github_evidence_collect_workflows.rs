// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    GitHubEvidenceDocumentKindV1, GitHubEvidenceRunAttemptV1, MAX_GITHUB_EVIDENCE_ATTEMPTS,
};

use super::api::{
    self, EvidenceResult, WorkflowAttemptDetail, WorkflowInventoryPage, WorkflowRunResponse,
    WorkflowRunsPage,
};
use super::validate::{validate_attempt, validate_run, workflow_slug};
use super::{Collector, EvidenceDocumentIdentity, MAX_API_PAGES, MAX_TOTAL_RUNS, PAGE_SIZE};
use crate::api::github_evidence_http::{EvidenceRoute, GitHubEvidenceRead};
use crate::internal::internal;

impl<R: GitHubEvidenceRead> Collector<'_, R> {
    pub(super) fn collect_workflow_inventory(&mut self) -> EvidenceResult<BTreeMap<String, u64>> {
        let mut selected = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut active_paths = BTreeMap::<String, Vec<u64>>::new();
        let mut expected_total = None;
        let mut page = 1;
        loop {
            if page > MAX_API_PAGES {
                return Err(internal("github_evidence_workflow_pages_limit"));
            }
            let (body, response): (Vec<u8>, WorkflowInventoryPage) =
                self.read_json(&EvidenceRoute::Workflows { page })?;
            if expected_total
                .replace(response.total_count)
                .is_some_and(|old| old != response.total_count)
                || api::expected_page_items(response.total_count, PAGE_SIZE, page)?
                    != response.workflows.len()
                || response.workflows.iter().any(|workflow| workflow.id == 0)
            {
                return Err(internal("github_evidence_workflow_inventory_incomplete"));
            }
            let document_path = format!("documents/workflows-page-{page:04}.json");
            self.capture_document(
                EvidenceDocumentIdentity {
                    kind: GitHubEvidenceDocumentKindV1::WorkflowInventory,
                    workflow_path: None,
                    run_id: None,
                    attempt: None,
                    artifact_id: None,
                    page: Some(page),
                    path: document_path,
                },
                &body,
            )?;
            for workflow in &response.workflows {
                if !ids.insert(workflow.id) {
                    return Err(internal("github_evidence_workflow_duplicate_id"));
                }
                if workflow.state == "active" {
                    active_paths
                        .entry(workflow.path.clone())
                        .or_default()
                        .push(workflow.id);
                }
            }
            let total = response.total_count;
            if page == api::expected_pages(total, PAGE_SIZE)? {
                break;
            }
            page += 1;
        }
        for source in &self.invocation.spec.workflow_sources {
            let candidates = active_paths
                .get(&source.path)
                .into_iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            if candidates.len() != 1
                || selected
                    .insert(source.path.clone(), candidates[0])
                    .is_some()
            {
                return Err(internal("github_evidence_workflow_identity_ambiguous"));
            }
        }
        self.workflow_inventory = selected.clone();
        Ok(selected)
    }

    pub(super) fn collect_workflows(
        &mut self,
        workflow_ids: &BTreeMap<String, u64>,
    ) -> EvidenceResult<Vec<GitHubEvidenceRunAttemptV1>> {
        let mut workflows = self.workflow_source_map().into_values().collect::<Vec<_>>();
        workflows.sort_by(|left, right| left.path.cmp(&right.path));
        let mut total_runs = 0_u64;
        for source in workflows {
            let workflow_id = *workflow_ids
                .get(&source.path)
                .ok_or_else(|| internal("github_evidence_workflow_id_missing"))?;
            let mut summaries = self.list_window_runs(&source.path, workflow_id)?;
            let predecessor = self.latest_predecessor(&source.path, workflow_id)?;
            summaries.push((predecessor, true));
            summaries.sort_by_key(|summary| summary.0.id);
            if summaries
                .windows(2)
                .any(|pair| pair[0].0.id == pair[1].0.id)
            {
                return Err(internal("github_evidence_run_duplicate"));
            }
            total_runs = total_runs.saturating_add(summaries.len() as u64);
            if total_runs > MAX_TOTAL_RUNS {
                return Err(internal("github_evidence_run_count_limit"));
            }
            for (summary, is_predecessor) in summaries {
                self.capture_run_attempts(
                    &source.path,
                    workflow_id,
                    &source,
                    &summary,
                    is_predecessor,
                )?;
            }
        }
        if self.attempts.len() > MAX_GITHUB_EVIDENCE_ATTEMPTS {
            return Err(internal("github_evidence_attempt_count_limit"));
        }
        Ok(self.attempts.clone())
    }

    fn list_window_runs(
        &mut self,
        workflow_path: &str,
        workflow_id: u64,
    ) -> EvidenceResult<Vec<(WorkflowRunResponse, bool)>> {
        let window = self.invocation.window;
        let created = format!("{}..{}", window.start_text(), window.query_end_text());
        let mut runs = Vec::new();
        let mut expected_total = None;
        let mut previous_created = None::<String>;
        let mut seen_ids = BTreeSet::new();
        let mut page = 1;
        loop {
            if page > MAX_API_PAGES {
                return Err(internal("github_evidence_run_pages_limit"));
            }
            let route = EvidenceRoute::WorkflowRuns {
                workflow_id,
                branch: self.invocation.spec.default_branch.clone(),
                created: created.clone(),
                page,
            };
            let (body, response): (Vec<u8>, WorkflowRunsPage) = self.read_json(&route)?;
            if response.total_count > MAX_TOTAL_RUNS
                || expected_total
                    .replace(response.total_count)
                    .is_some_and(|old| old != response.total_count)
                || api::expected_page_items(response.total_count, PAGE_SIZE, page)?
                    != response.workflow_runs.len()
            {
                return Err(internal("github_evidence_run_pages_incomplete"));
            }
            let document_path = format!(
                "documents/runs-{}-page-{page:04}.json",
                workflow_slug(workflow_path)?
            );
            self.capture_document(
                EvidenceDocumentIdentity {
                    kind: GitHubEvidenceDocumentKindV1::WorkflowRunsPage,
                    workflow_path: Some(workflow_path.to_owned()),
                    run_id: None,
                    attempt: None,
                    artifact_id: None,
                    page: Some(page),
                    path: document_path,
                },
                &body,
            )?;
            for run in response.workflow_runs {
                validate_run(&run, &self.invocation, workflow_id, true)?;
                if previous_created
                    .as_ref()
                    .is_some_and(|created| created < &run.created_at)
                    || !seen_ids.insert(run.id)
                {
                    return Err(internal("github_evidence_run_order_or_duplicate"));
                }
                previous_created = Some(run.created_at.clone());
                runs.push((run, false));
            }
            let total = expected_total.unwrap_or_default();
            if page == api::expected_pages(total, PAGE_SIZE)? {
                break;
            }
            page += 1;
        }
        Ok(runs)
    }

    fn latest_predecessor(
        &mut self,
        workflow_path: &str,
        workflow_id: u64,
    ) -> EvidenceResult<WorkflowRunResponse> {
        let route = EvidenceRoute::WorkflowPredecessor {
            workflow_id,
            branch: self.invocation.spec.default_branch.clone(),
            created_before: self.invocation.window.predecessor_cutoff_text(),
        };
        let (body, response): (Vec<u8>, WorkflowRunsPage) = self.read_json(&route)?;
        if response.total_count == 0
            || response.workflow_runs.len() != 1
            || api::expected_page_items(response.total_count, 1, 1)? != 1
        {
            return Err(internal("github_evidence_predecessor_missing"));
        }
        self.capture_document(
            EvidenceDocumentIdentity {
                kind: GitHubEvidenceDocumentKindV1::WorkflowPredecessor,
                workflow_path: Some(workflow_path.to_owned()),
                run_id: None,
                attempt: None,
                artifact_id: None,
                page: Some(1),
                path: format!(
                    "documents/predecessor-{}.json",
                    workflow_slug(workflow_path)?
                ),
            },
            &body,
        )?;
        let run = response
            .workflow_runs
            .into_iter()
            .next()
            .ok_or_else(|| internal("github_evidence_predecessor_missing"))?;
        validate_run(&run, &self.invocation, workflow_id, false)?;
        if run.created_at >= self.invocation.window.start_text() {
            return Err(internal("github_evidence_predecessor_not_before_window"));
        }
        Ok(run)
    }

    fn capture_run_attempts(
        &mut self,
        workflow_path: &str,
        workflow_id: u64,
        source: &velnor_actions_contract::GitHubEvidenceWorkflowRefV1,
        summary: &WorkflowRunResponse,
        is_predecessor: bool,
    ) -> EvidenceResult<()> {
        if summary.run_attempt == 0
            || self
                .attempts
                .len()
                .saturating_add(summary.run_attempt as usize)
                > MAX_GITHUB_EVIDENCE_ATTEMPTS
        {
            return Err(internal("github_evidence_attempt_count_limit"));
        }
        for attempt in 1..=summary.run_attempt {
            self.check_deadline()?;
            let (body, detail): (Vec<u8>, WorkflowAttemptDetail) =
                self.read_json(&EvidenceRoute::RunAttempt {
                    run_id: summary.id,
                    attempt,
                })?;
            validate_attempt(&detail, summary, workflow_id, attempt)?;
            self.capture_document(
                EvidenceDocumentIdentity {
                    kind: GitHubEvidenceDocumentKindV1::RunAttempt,
                    workflow_path: Some(workflow_path.to_owned()),
                    run_id: Some(summary.id),
                    attempt: Some(attempt),
                    artifact_id: None,
                    page: None,
                    path: format!(
                        "documents/attempt-{}-{}-{attempt}.json",
                        workflow_slug(workflow_path)?,
                        summary.id
                    ),
                },
                &body,
            )?;
            let jobs_documents = self.capture_jobs(
                workflow_path,
                source,
                summary.id,
                attempt,
                &summary.head_sha,
            )?;
            self.attempts.push(GitHubEvidenceRunAttemptV1 {
                workflow_path: workflow_path.to_owned(),
                run_id: summary.id,
                attempt,
                created_at: detail.created_at,
                is_predecessor,
                event: detail.event,
                head_branch: detail.head_branch,
                head_sha: detail.head_sha,
                status: detail.status,
                conclusion: detail.conclusion,
                jobs_documents,
            });
        }
        Ok(())
    }

    fn capture_jobs(
        &mut self,
        workflow_path: &str,
        source: &velnor_actions_contract::GitHubEvidenceWorkflowRefV1,
        run_id: u64,
        attempt: u32,
        head_sha: &str,
    ) -> EvidenceResult<Vec<u32>> {
        let mut expected_total = None;
        let mut job_ids = BTreeSet::new();
        let mut named_task_job_count = 0;
        let mut document_indexes = Vec::new();
        let mut page = 1;
        loop {
            if page > MAX_API_PAGES {
                return Err(internal("github_evidence_jobs_pages_limit"));
            }
            let route = EvidenceRoute::Jobs {
                run_id,
                attempt,
                page,
            };
            let (body, response): (Vec<u8>, api::JobsPage) = self.read_json(&route)?;
            if expected_total
                .replace(response.total_count)
                .is_some_and(|old| old != response.total_count)
                || api::expected_page_items(response.total_count, PAGE_SIZE, page)?
                    != response.jobs.len()
            {
                return Err(internal("github_evidence_jobs_pages_incomplete"));
            }
            for job in &response.jobs {
                if job.id == 0
                    || !job_ids.insert(job.id)
                    || job.name.is_empty()
                    || job.run_id != run_id
                    || job.head_sha != head_sha
                {
                    return Err(internal("github_evidence_jobs_identity"));
                }
                if source.job_name.as_deref() == Some(job.name.as_str()) {
                    named_task_job_count += 1;
                }
            }
            let index = self.capture_document(
                EvidenceDocumentIdentity {
                    kind: GitHubEvidenceDocumentKindV1::RunAttemptJobsPage,
                    workflow_path: Some(workflow_path.to_owned()),
                    run_id: Some(run_id),
                    attempt: Some(attempt),
                    artifact_id: None,
                    page: Some(page),
                    path: format!(
                        "documents/jobs-{}-{run_id}-{attempt}-{page:04}.json",
                        workflow_slug(workflow_path)?
                    ),
                },
                &body,
            )?;
            document_indexes.push(index);
            let total = expected_total.unwrap_or_default();
            if page == api::expected_pages(total, PAGE_SIZE)? {
                break;
            }
            page += 1;
        }
        if source.job_name.is_some() && named_task_job_count != 1 {
            return Err(internal("github_evidence_suite_job_missing_or_ambiguous"));
        }
        Ok(document_indexes)
    }
}
