// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::dispatch::WorkflowDispatch;
use super::jobs::is_safe_display_name;
use super::permissions::{PermissionLevel, Permissions};
pub use super::step::{Step, StepKind};
#[path = "job_validation.rs"]
mod job_validation;
use super::timeout::JobTimeout;
use crate::errors::ContractError;
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
pub struct Concurrency {
    pub group: String,

    pub cancel_in_progress: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheMode {
    Read,

    Write,

    WriteOnly,

    None,

    #[serde(untagged)]
    Expression(String),
}
impl CacheMode {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::WriteOnly => "write-only",
            Self::None => "none",
            Self::Expression(expr) => expr.as_str(),
        }
    }

    #[must_use]
    pub fn push_write_expression() -> Self {
        Self::Expression(CACHE_MODE_PUSH_WRITE_EXPR.to_owned())
    }

    pub fn parse(value: &str) -> Result<Self, ContractError> {
        let mode = match value {
            "read" => Self::Read,
            "write" => Self::Write,
            "write-only" => Self::WriteOnly,
            "none" => Self::None,
            expr if expr.starts_with("${{") && expr.ends_with("}}") => {
                Self::Expression(expr.to_owned())
            }
            _ => {
                return Err(ContractError::identity(
                    "cache_mode",
                    format!("invalid_cache_mode:{value}"),
                ));
            }
        };
        mode.validate("cache_mode")?;
        Ok(mode)
    }

    pub fn validate(&self, field: &'static str) -> Result<(), ContractError> {
        if let Self::Expression(expr) = self {
            let inner = expr
                .strip_prefix("${{")
                .and_then(|rest| rest.strip_suffix("}}"))
                .map(str::trim);
            if inner.is_none_or(str::is_empty) || expr.chars().any(char::is_control) {
                return Err(ContractError::identity(field, "bad_cache_mode_expression"));
            }
        }
        Ok(())
    }
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

    pub steps: Vec<Step>,
}

pub const CACHE_SAVE_CONDITION: &str = "success() && github.event_name == 'push'";

pub const CACHE_MODE_PUSH_WRITE_EXPR: &str =
    "${{ github.event_name == 'push' && 'write' || 'read' }}";

impl WorkflowIr {
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
        for (id, job) in &self.jobs {
            job.validate(id, &ids, &self.permissions, pr_triggered)?;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleTrigger {
    pub cron: Vec<String>,
}

impl ScheduleTrigger {
    #[must_use]
    pub fn new(cron: Vec<String>) -> Self {
        Self { cron }
    }

    #[must_use]
    pub fn single(expr: impl Into<String>) -> Self {
        Self {
            cron: vec![expr.into()],
        }
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.cron.is_empty() {
            return Err(ContractError::identity("schedule.cron", "empty_cron"));
        }
        let bounds: [(u32, u32); 5] = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)];
        for entry in &self.cron {
            let fields: Vec<&str> = entry.split_whitespace().collect();
            let valid = fields.len() == 5
                && fields
                    .into_iter()
                    .zip(bounds)
                    .all(|(field, (min, max))| is_valid_cron_field(field, min, max));
            if !valid {
                return Err(ContractError::identity(
                    "schedule.cron",
                    format!("bad_cron:{entry}"),
                ));
            }
        }
        Ok(())
    }
}

fn is_valid_cron_field(field: &str, min: u32, max: u32) -> bool {
    !field.is_empty()
        && field
            .split(',')
            .all(|part| is_valid_cron_part(part, min, max))
}

fn is_valid_cron_part(part: &str, min: u32, max: u32) -> bool {
    if part.is_empty() {
        return false;
    }
    let (range_part, step_ok) = match part.split_once('/') {
        Some((base, step)) => (
            base,
            parse_cron_num(step).is_some_and(|s| s >= 1 && s <= max.max(1)),
        ),
        None => (part, true),
    };
    if !step_ok || range_part.is_empty() {
        return false;
    }
    if range_part == "*" {
        return true;
    }
    if let Some((start_str, end_str)) = range_part.split_once('-') {
        let Some(start) = parse_cron_num(start_str) else {
            return false;
        };
        let Some(end) = parse_cron_num(end_str) else {
            return false;
        };
        start >= min && end <= max && start <= end
    } else {
        parse_cron_num(range_part).is_some_and(|n| n >= min && n <= max)
    }
}

fn parse_cron_num(text: &str) -> Option<u32> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse::<u32>().ok())
        .flatten()
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
