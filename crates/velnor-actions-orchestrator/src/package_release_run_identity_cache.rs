use super::{RunSource, WorkflowMetadata};
use crate::{OrchestratorError, internal::internal};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(in crate::package_release_publish) struct RunIdentityCache {
    workflows: Vec<(u64, String, String)>,
    completed: BTreeSet<CompletedAttempt>,
    active_jobs: BTreeMap<(CompletedAttempt, String), u64>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CompletedAttempt {
    repository_id: u64,
    source: String,
    branch: String,
    workflow_id: u64,
    workflow_path: String,
    workflow_name: String,
    run_id: u64,
    attempt: u32,
}

impl CompletedAttempt {
    pub(super) fn new(
        id: u64,
        attempt: u32,
        source: RunSource<'_>,
        workflow: &WorkflowMetadata,
    ) -> Self {
        Self {
            repository_id: source.repository_id,
            source: source.sha.to_owned(),
            branch: source.branch.to_owned(),
            workflow_id: workflow.id,
            workflow_path: workflow.path.clone(),
            workflow_name: workflow.name.clone(),
            run_id: id,
            attempt,
        }
    }
}

impl RunIdentityCache {
    pub(super) fn workflow(&self, path: &str, name: &str) -> Option<WorkflowMetadata> {
        self.workflows
            .iter()
            .find(|(_, p, n)| p == path && n == name)
            .map(|(id, path, name)| WorkflowMetadata {
                id: *id,
                path: path.clone(),
                name: name.clone(),
                state: "active".to_owned(),
            })
    }

    pub(super) fn admit_workflow(
        &mut self,
        workflow: &WorkflowMetadata,
    ) -> Result<(), OrchestratorError> {
        if self.workflows.len() >= 16 {
            return Err(internal("package_workflow_cache_bound"));
        }
        self.workflows
            .push((workflow.id, workflow.path.clone(), workflow.name.clone()));
        Ok(())
    }

    pub(super) fn contains(&self, attempt: &CompletedAttempt) -> bool {
        self.completed.contains(attempt)
    }

    pub(super) fn admit_completed(
        &mut self,
        attempt: CompletedAttempt,
    ) -> Result<(), OrchestratorError> {
        if self.completed.len() >= 64 {
            return Err(internal("package_ci_cache_bound"));
        }
        self.completed.insert(attempt);
        Ok(())
    }
}

impl RunIdentityCache {
    pub(super) fn active_job(&self, key: &CompletedAttempt, name: &str) -> Option<u64> {
        self.active_jobs
            .get(&(key.clone(), name.to_owned()))
            .copied()
    }
    pub(super) fn admit_active_job(
        &mut self,
        key: CompletedAttempt,
        name: &str,
        id: u64,
    ) -> Result<(), OrchestratorError> {
        if self.active_jobs.len() >= 64 {
            return Err(internal("package_active_job_cache_bound"));
        }
        self.active_jobs.insert((key, name.to_owned()), id);
        Ok(())
    }
}
