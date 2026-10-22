use super::super::github::{WorkflowJob, WorkflowJobList};
use crate::{OrchestratorError, internal::internal};
use serde::Deserialize;
use std::collections::BTreeSet;

pub(super) const WORKFLOW_JOBS_PAGE_SIZE: usize = 100;
pub(super) const MAX_WORKFLOW_JOB_PAGES: usize = 20;
const MAX_WORKFLOW_JOBS: usize = WORKFLOW_JOBS_PAGE_SIZE * MAX_WORKFLOW_JOB_PAGES;

#[derive(Deserialize)]
pub(super) struct WorkflowJobsPage {
    pub(super) total_count: usize,
    pub(super) jobs: Vec<IdentifiedWorkflowJob>,
}

#[derive(Deserialize)]
pub(super) struct IdentifiedWorkflowJob {
    pub(super) id: u64,
    pub(super) run_id: u64,
    pub(super) name: String,
    pub(super) head_sha: Option<String>,
    pub(super) status: Option<String>,
    pub(super) conclusion: Option<String>,
}

#[derive(Default)]
pub(super) struct WorkflowJobsAccumulator {
    total_count: Option<usize>,
    ids: BTreeSet<u64>,
    jobs: Vec<WorkflowJob>,
}

impl WorkflowJobsAccumulator {
    pub(super) fn admit_page(&mut self, page: WorkflowJobsPage) -> Result<bool, OrchestratorError> {
        let page_len = page.jobs.len();
        if page.jobs.len() > WORKFLOW_JOBS_PAGE_SIZE
            || page.total_count > MAX_WORKFLOW_JOBS
            || self
                .total_count
                .is_some_and(|total| total != page.total_count)
        {
            return Err(internal("package_ci_job_inventory_count"));
        }
        self.total_count = Some(page.total_count);
        for job in page.jobs {
            if job.id == 0 || !self.ids.insert(job.id) {
                return Err(internal("package_ci_job_inventory_identity"));
            }
            self.jobs.push(WorkflowJob {
                run_id: job.run_id,
                name: job.name,
                head_sha: job.head_sha,
                status: job.status,
                conclusion: job.conclusion,
            });
        }
        let total = page.total_count;
        if self.jobs.len() > total {
            return Err(internal("package_ci_job_inventory_count"));
        }
        let complete = self.jobs.len() == total;
        if !complete && page_len < WORKFLOW_JOBS_PAGE_SIZE {
            return Err(internal("package_ci_job_inventory_truncated"));
        }
        Ok(complete)
    }

    pub(super) fn into_list(self) -> WorkflowJobList {
        WorkflowJobList { jobs: self.jobs }
    }
}
