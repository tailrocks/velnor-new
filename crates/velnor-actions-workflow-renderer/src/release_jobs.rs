// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{JobTimeout, Step, StepKind, validate_job_id};

use crate::{
    RenderError,
    release_permissions::JobPermissions,
    release_spec::{
        ReleaseConcurrency, ReleasePlan, ReleaseTriggers, check_lock_anchor, is_clean_text,
        publish_gate_condition, validate_environment, validate_repository,
    },
    steps::scan_for_private_subcommands,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReleaseRole {
    Preparation,
    PrCreate,
    Preflight,
    PublishOidc,
    PublishBootstrap,
    Reconcile,
}

impl ReleaseRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preparation => "preparation",
            Self::PrCreate => "pr-create",
            Self::Preflight => "preflight",
            Self::PublishOidc => "publish-oidc",
            Self::PublishBootstrap => "publish-bootstrap",
            Self::Reconcile => "reconcile",
        }
    }

    #[must_use]
    pub fn is_validation(self) -> bool {
        matches!(self, Self::Preflight | Self::Reconcile)
    }

    fn rank(self) -> u8 {
        match self {
            Self::Preflight => 0,
            Self::Preparation | Self::PublishOidc | Self::PublishBootstrap => 1,
            Self::PrCreate | Self::Reconcile => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReleaseJobSpec {
    pub role: ReleaseRole,
    pub display_name: String,
    pub runs_on: String,
    pub timeout_minutes: JobTimeout,
    pub needs: Vec<String>,
    pub outputs: BTreeMap<String, String>,
    pub condition: Option<String>,
    pub environment: Option<String>,
    pub permissions: JobPermissions,
    pub steps: Vec<Step>,
}

fn check_condition_text(condition: &str) -> Result<(), RenderError> {
    if !is_clean_text(condition, 512) || condition.contains("secrets.") {
        return Err(RenderError::InvalidWorkflow(format!(
            "bad_condition:{condition}"
        )));
    }
    scan_for_private_subcommands(condition)
}

impl ReleaseJobSpec {
    fn validate_outputs(&self, id: &str) -> Result<(), RenderError> {
        if self.role == ReleaseRole::Preflight
            && (self.outputs.len() != 4
                || ![
                    "source_sha",
                    "eligible",
                    "rust_artifact_id",
                    "receipt_artifact_id",
                ]
                .iter()
                .all(|key| self.outputs.contains_key(*key)))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "missing_release_output:{id}"
            )));
        }
        if self.role == ReleaseRole::Preparation && !self.outputs.is_empty() {
            let expected = BTreeMap::from([
                (
                    "artifact_id".to_owned(),
                    "${{ steps.release-pr-artifact.outputs.artifact-id }}".to_owned(),
                ),
                (
                    "artifact_digest".to_owned(),
                    "${{ steps.release-pr-artifact.outputs.artifact-digest }}".to_owned(),
                ),
                (
                    "content_sha256".to_owned(),
                    "${{ steps.release-pr-candidate.outputs.content_sha256 }}".to_owned(),
                ),
                (
                    "changed".to_owned(),
                    "${{ steps.release-pr-candidate.outputs.changed }}".to_owned(),
                ),
            ]);
            if self.outputs != expected {
                return Err(RenderError::InvalidWorkflow(format!(
                    "bad_preparation_output:{id}"
                )));
            }
        } else if self.outputs.iter().any(|(key, value)| {
            if key == "receipt_artifact_id" {
                self.role != ReleaseRole::Preflight
                    || value != "${{ steps.release-receipt-upload.outputs.artifact-id }}"
            } else if key == "rust_artifact_id" {
                self.role != ReleaseRole::Preflight
                    || value != "${{ steps.release-rust-upload.outputs.artifact-id }}"
            } else {
                !matches!(key.as_str(), "source_sha" | "eligible")
                    || value != &format!("${{{{ steps.release-admission.outputs.{key} }}}}")
                    || !matches!(
                        self.role,
                        ReleaseRole::Preflight
                            | ReleaseRole::PublishOidc
                            | ReleaseRole::PublishBootstrap
                    )
            }
        }) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_release_output:{id}"
            )));
        }
        Ok(())
    }

    pub fn validate_shape(&self, id: &str) -> Result<(), RenderError> {
        self.validate_outputs(id)?;
        if !is_clean_text(&self.display_name, 128) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_job_display:{id}"
            )));
        }
        scan_for_private_subcommands(&self.display_name)?;
        crate::guard::validate_runs_on(&self.runs_on)?;
        if self.steps.is_empty() {
            return Err(RenderError::InvalidWorkflow(format!("empty_steps:{id}")));
        }
        for step in &self.steps {
            if !is_clean_text(&step.name, 128) {
                return Err(RenderError::InvalidWorkflow(format!("bad_step_name:{id}")));
            }
            if matches!(step.kind, StepKind::Internal { .. }) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_internal_op:{id}"
                )));
            }
            scan_for_private_subcommands(&step.name)?;
        }
        if let Some(environment) = &self.environment {
            validate_environment(environment)?;
        }
        self.permissions
            .validate(self.role, self.environment.as_deref())?;
        if let Some(condition) = &self.condition {
            check_condition_text(condition)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ReleaseWorkflowSpec {
    pub name: String,
    pub repository: String,
    pub default_branch: String,
    pub triggers: ReleaseTriggers,
    pub concurrency: ReleaseConcurrency,
    pub jobs: BTreeMap<String, ReleaseJobSpec>,
    pub plan: ReleasePlan,
    pub publish_environment: String,
    pub bootstrap_environment: String,
    pub release_plz_version: String,
    pub journal_predecessor: Option<velnor_actions_contract::config::CrateReleasePredecessor>,
    pub new_crates: Option<velnor_actions_contract::config::NewCrateRelease>,
}

impl ReleaseWorkflowSpec {
    pub fn validate(&self) -> Result<(), RenderError> {
        if !is_clean_text(&self.name, 128) {
            return Err(RenderError::InvalidWorkflow("bad_release_name".to_owned()));
        }
        scan_for_private_subcommands(&self.name)?;
        validate_repository(&self.repository)?;
        self.plan.validate()?;
        if self.plan.repository != self.repository {
            return Err(RenderError::InvalidWorkflow(
                "bootstrap_repository_mismatch".to_owned(),
            ));
        }
        validate_environment(&self.publish_environment)?;
        validate_environment(&self.bootstrap_environment)?;
        self.triggers.validate(&self.plan)?;
        if !velnor_actions_contract::is_valid_branch_name(&self.default_branch)
            || (!self.plan.dispatch
                && self.triggers.push_branches.as_slice() != [self.default_branch.as_str()])
        {
            return Err(RenderError::InvalidWorkflow(
                "release_default_branch_mismatch".to_owned(),
            ));
        }
        self.concurrency.validate()?;
        check_lock_anchor(&self.concurrency.group, &self.repository)?;
        check_role_set(&self.jobs)?;
        for (id, job) in &self.jobs {
            job.validate_shape(id)?;
        }
        check_needs_graph(&self.jobs)?;
        check_role_conditions(
            &self.jobs,
            &self.repository,
            &self.default_branch,
            &self.plan,
        )?;
        check_publish_needs(&self.jobs)?;
        Ok(())
    }
}

#[path = "release_jobs_graph.rs"]
mod graph;
use graph::{check_needs_graph, check_publish_needs, check_role_conditions, check_role_set};
