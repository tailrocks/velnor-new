// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::dispatch::WorkflowDispatch;
use super::jobs::is_safe_display_name;
use super::permissions::{PermissionLevel, Permissions};
pub use super::step::{Step, StepKind};
mod cache_mode;
mod concurrency;
#[path = "job_validation.rs"]
mod job_validation;
mod schedule;
use super::timeout::JobTimeout;
use crate::errors::ContractError;
pub use cache_mode::CacheMode;
pub use concurrency::{Concurrency, WRITER_QUEUE_CANCEL_IN_PROGRESS};
pub use schedule::ScheduleTrigger;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowIr {
    pub name: String,

    pub triggers: Trigger,

    pub permissions: Permissions,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_mode: Option<CacheMode>,

    pub concurrency: Concurrency,

    pub jobs: BTreeMap<String, Job>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trigger {
    pub pull_request_types: Vec<String>,

    pub push_branches: Vec<String>,

    pub merge_group: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_dispatch: Option<WorkflowDispatch>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ScheduleTrigger>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub display_name: String,

    pub runs_on: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_runner: Option<crate::config::CheckRunner>,

    pub timeout_minutes: JobTimeout,

    #[serde(default)]
    pub needs: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_mode: Option<CacheMode>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<super::services::ServiceContainer>,

    /// Job-level outputs (`jobs.<id>.outputs`) published for `needs` readers.
    /// Empty for jobs that publish nothing; the branch reader publishes the
    /// bundle key outputs so the writer recheck can restore by the same key
    /// before the writer's own key step runs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, String>,

    pub steps: Vec<Step>,
}

pub const CACHE_SAVE_CONDITION: &str = "success() && github.event_name == 'push'";

pub const CACHE_MODE_PUSH_WRITE_EXPR: &str =
    "${{ github.event_name == 'push' && 'write' || 'read' }}";

impl Job {
    #[must_use]
    pub fn uses_checkout(&self) -> bool {
        self.steps.iter().any(|step| {
            matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@"))
        })
    }
}

impl WorkflowIr {
    pub fn validate_event_authority(
        &self,
        readers: &BTreeSet<&str>,
        writers: &BTreeSet<&str>,
        cleanup: &BTreeSet<&str>,
    ) -> Result<(), ContractError> {
        for id in readers {
            let job = self.jobs.get(*id).ok_or_else(|| {
                ContractError::identity("job.authority", format!("unknown_reader:{id}"))
            })?;
            if !job
                .cache_mode
                .as_ref()
                .is_some_and(CacheMode::is_explicit_read)
            {
                return Err(ContractError::identity(
                    "job.cache_mode",
                    format!("reader_requires_explicit_read:{id}"),
                ));
            }
        }
        for id in writers {
            if !self.jobs.contains_key(*id) {
                return Err(ContractError::identity(
                    "job.authority",
                    format!("unknown_writer:{id}"),
                ));
            }
        }
        for (id, job) in &self.jobs {
            if writers.contains(id.as_str()) {
                continue;
            }
            if job.cache_mode.as_ref().is_some_and(CacheMode::grants_write) {
                return Err(ContractError::identity(
                    "job.cache_mode",
                    format!("cache_write_outside_trusted_writer:{id}"),
                ));
            }
        }
        for id in cleanup {
            let job = self.jobs.get(*id).ok_or_else(|| {
                ContractError::identity("job.authority", format!("unknown_cleanup:{id}"))
            })?;
            let effective = job.permissions.as_ref().unwrap_or(&self.permissions);
            if !matches!(effective.actions, PermissionLevel::Write) {
                return Err(ContractError::identity(
                    "job.permissions",
                    format!("cleanup_requires_actions_write:{id}"),
                ));
            }
            if job.uses_checkout() {
                return Err(ContractError::identity(
                    "job.permissions",
                    format!("actions_write_requires_checkout_free:{id}"),
                ));
            }
        }
        for (id, job) in &self.jobs {
            if cleanup.contains(id.as_str()) {
                continue;
            }
            let effective = job.permissions.as_ref().unwrap_or(&self.permissions);
            if matches!(effective.actions, PermissionLevel::Write) {
                return Err(ContractError::identity(
                    "job.permissions",
                    format!("actions_write_outside_cleanup:{id}"),
                ));
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.name.trim().is_empty() {
            return Err(ContractError::identity("workflow.name", "empty_name"));
        }
        if self.permissions.is_write_all() {
            return Err(ContractError::identity("workflow.permissions", "write_all"));
        }
        if let Some(cache_mode) = &self.cache_mode {
            cache_mode.validate("workflow.cache_mode")?;
        }
        self.triggers.validate()?;
        if self.concurrency.group.trim().is_empty() {
            return Err(ContractError::identity(
                "workflow.concurrency",
                "empty_group",
            ));
        }
        if self.jobs.is_empty() {
            return Err(ContractError::identity("workflow.jobs", "empty_jobs"));
        }
        let ids: BTreeSet<&str> = self.jobs.keys().map(String::as_str).collect();
        let pr_triggered = !self.triggers.pull_request_types.is_empty();
        let untrusted = pr_triggered || self.triggers.merge_group;
        for (id, job) in &self.jobs {
            job.validate(id, &ids, &self.permissions, pr_triggered)?;
            if untrusted
                && job
                    .cache_mode
                    .as_ref()
                    .is_some_and(CacheMode::grants_unconditional_write)
            {
                return Err(ContractError::identity(
                    "job.cache_mode",
                    format!("cache_write_outside_trusted_writer:{id}"),
                ));
            }
            let effective = job.permissions.as_ref().unwrap_or(&self.permissions);
            if matches!(effective.actions, PermissionLevel::Write) && job.uses_checkout() {
                return Err(ContractError::identity(
                    "job.permissions",
                    format!("actions_write_requires_checkout_free:{id}"),
                ));
            }
        }
        if untrusted
            && self
                .cache_mode
                .as_ref()
                .is_some_and(CacheMode::grants_unconditional_write)
        {
            return Err(ContractError::identity(
                "workflow.cache_mode",
                "cache_write_outside_trusted_writer",
            ));
        }
        Ok(())
    }
}
impl Trigger {
    fn validate(&self) -> Result<(), ContractError> {
        for pr_type in &self.pull_request_types {
            if super::workflow_policy::is_restricted_action_event(pr_type) {
                return Err(ContractError::identity(
                    "trigger.pull_request_types",
                    format!("restricted_untrusted_actor_event:{pr_type}"),
                ));
            }
        }
        for branch in &self.push_branches {
            if !crate::is_valid_branch_name(branch) {
                return Err(ContractError::identity(
                    "trigger.push_branches",
                    format!("malformed_branch:{branch}"),
                ));
            }
        }
        if let Some(dispatch) = &self.workflow_dispatch {
            dispatch.validate()?;
        }
        if let Some(schedule) = &self.schedule {
            schedule.validate()?;
        }
        Ok(())
    }
}

fn is_valid_environment(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'/' | b'.'))
        && !name.split('/').any(|seg| seg.is_empty() || seg == "..")
}

fn is_pinned_label(label: &str) -> bool {
    crate::config::is_legacy_hosted_label(label)
        || crate::config::ScaleSetSelector::parse_token(label).is_ok()
}
