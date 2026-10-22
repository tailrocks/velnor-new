//! Closed, credential-free companion workflows for explicitly declared Mise tasks.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{
    MiseTaskSource, RustSuiteArtifact, VerificationRunner, is_valid_mise_task_name,
    is_valid_workflow_task_id,
};
use crate::{ContractError, workflow::timeout::JobTimeout};

pub(super) fn validate_rust_suite_slug_collisions(
    rust_suites: &[super::RustSuiteConfig],
    task_suites: &[MiseTaskSuite],
    file: &str,
) -> Result<(), ContractError> {
    for suite in task_suites {
        if rust_suites.iter().any(|rust_suite| {
            rust_suite
                .workflow
                .as_deref()
                .unwrap_or(super::DEFAULT_SUITE_WORKFLOW_SLUG)
                == suite.workflow
        }) {
            let path = format!(".github/workflows/{}.yml", suite.workflow);
            return Err(ContractError::config(
                file,
                "workflow.task_suites",
                format!("workflow_slug_shared_across_suite_types:{path}"),
            ));
        }
    }
    Ok(())
}

/// Events that may start a standalone Mise task suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiseTaskSuiteEvent {
    /// Pushes to the configured default branch.
    Push,
    /// An explicitly declared schedule.
    Schedule,
}

impl MiseTaskSuiteEvent {
    /// GitHub Actions event name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Push => "push",
            Self::Schedule => "schedule",
        }
    }
}

/// One bounded, credential-free task suite under `[[workflow.task_suites]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MiseTaskSuite {
    /// Stable lowercase identifier used in job and artifact identities.
    pub id: String,
    /// Exact task name from the locked Mise configuration.
    pub mise_task: String,
    /// Exact Mise source and working-directory binding.
    pub source: MiseTaskSource,
    /// Fixed operating-system and architecture pair.
    pub runner: VerificationRunner,
    /// Bounded per-job timeout in minutes.
    pub timeout_minutes: u16,
    /// Generated workflow filename slug, without `.yml`.
    pub workflow: String,
    /// Explicit, non-empty set of allowed events.
    pub events: Vec<MiseTaskSuiteEvent>,
    /// Cron expressions required when `events` contains `schedule`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedule: Vec<String>,
    /// Declared evidence artifacts. Names are fixed per attempt by the renderer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<RustSuiteArtifact>,
}

impl MiseTaskSuite {
    /// Validate task identity, source, event, workflow, and artifact bounds.
    /// # Errors
    pub fn validate(&self, file: &str, prefix: &str) -> Result<(), ContractError> {
        self.validate_identity_and_limits(file, prefix)?;
        self.validate_events(file, prefix)?;
        self.validate_artifacts(file, prefix)
    }

    fn validate_identity_and_limits(&self, file: &str, prefix: &str) -> Result<(), ContractError> {
        if !is_valid_workflow_task_id(&self.id) {
            return Err(ContractError::config(
                file,
                format!("{prefix}.id"),
                format!("bad_workflow_task_id:{}", self.id),
            ));
        }
        if !is_valid_mise_task_name(&self.mise_task) {
            return Err(ContractError::config(
                file,
                format!("{prefix}.mise_task"),
                format!("bad_mise_task:{}", self.mise_task),
            ));
        }
        if !self.source.validate() {
            return Err(ContractError::config(
                file,
                format!("{prefix}.source"),
                "bad_mise_task_suite_source_or_working_directory",
            ));
        }
        if self.timeout_minutes == 0 || self.timeout_minutes > JobTimeout::MAX_MINUTES {
            return Err(ContractError::config(
                file,
                format!("{prefix}.timeout_minutes"),
                format!("bad_task_suite_timeout:{}", self.timeout_minutes),
            ));
        }
        if !valid_workflow_slug(&self.workflow)
            || self.workflow == "suites"
            || super::RESERVED_SUITE_WORKFLOW_SLUGS.contains(&self.workflow.as_str())
        {
            return Err(ContractError::config(
                file,
                format!("{prefix}.workflow"),
                format!("reserved_or_bad_task_suite_workflow:{}", self.workflow),
            ));
        }
        Ok(())
    }

    fn validate_events(&self, file: &str, prefix: &str) -> Result<(), ContractError> {
        if self.events.is_empty() {
            return Err(ContractError::config(
                file,
                format!("{prefix}.events"),
                "empty_task_suite_events",
            ));
        }
        let mut seen_events = BTreeSet::new();
        for event in &self.events {
            if !seen_events.insert(*event) {
                return Err(ContractError::config(
                    file,
                    format!("{prefix}.events"),
                    format!("duplicate_task_suite_event:{}", event.as_str()),
                ));
            }
        }
        if self.events.contains(&MiseTaskSuiteEvent::Schedule) {
            return self.validate_schedule(file, prefix);
        }
        if !self.schedule.is_empty() {
            return Err(ContractError::config(
                file,
                format!("{prefix}.schedule"),
                "schedule_requires_schedule_event",
            ));
        }
        Ok(())
    }

    fn validate_schedule(&self, file: &str, prefix: &str) -> Result<(), ContractError> {
        if self.schedule.is_empty() {
            return Err(ContractError::config(
                file,
                format!("{prefix}.schedule"),
                "empty_task_suite_schedule",
            ));
        }
        let mut seen = BTreeSet::new();
        for cron in &self.schedule {
            if !seen.insert(cron) {
                return Err(ContractError::config(
                    file,
                    format!("{prefix}.schedule"),
                    format!("duplicate_task_suite_cron:{cron}"),
                ));
            }
            crate::workflow::ScheduleTrigger::single(cron.clone())
                .validate()
                .map_err(|_| {
                    ContractError::config(
                        file,
                        format!("{prefix}.schedule"),
                        format!("bad_cron:{cron}"),
                    )
                })?;
        }
        Ok(())
    }

    fn validate_artifacts(&self, file: &str, prefix: &str) -> Result<(), ContractError> {
        let mut artifact_names = BTreeSet::new();
        for (index, artifact) in self.artifacts.iter().enumerate() {
            if !artifact_names.insert(artifact.name.as_str()) {
                return Err(ContractError::config(
                    file,
                    format!("{prefix}.artifacts[{index}].name"),
                    format!("duplicate_task_suite_artifact:{}", artifact.name),
                ));
            }
            super::rust_suites_validate::validate_task_suite_artifact(
                artifact,
                file,
                &format!("{prefix}.artifacts[{index}]"),
            )?;
        }
        Ok(())
    }
}

fn valid_workflow_slug(slug: &str) -> bool {
    let mut bytes = slug.bytes();
    matches!(bytes.next(), Some(byte) if byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}
