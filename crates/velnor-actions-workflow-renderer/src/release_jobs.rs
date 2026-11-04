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
            &self.triggers.push_branches[0],
            &self.plan,
        )?;
        check_publish_needs(&self.jobs)?;
        Ok(())
    }
}

fn check_role_set(jobs: &BTreeMap<String, ReleaseJobSpec>) -> Result<(), RenderError> {
    let mut roles: Vec<ReleaseRole> = Vec::with_capacity(jobs.len());
    for (id, job) in jobs {
        validate_job_id(id).map_err(RenderError::Contract)?;
        roles.push(job.role);
    }
    roles.sort();
    let mut required = vec![
        ReleaseRole::Preparation,
        ReleaseRole::Preflight,
        ReleaseRole::PublishOidc,
        ReleaseRole::Reconcile,
    ];
    required.sort();
    let mut with_bootstrap = vec![
        ReleaseRole::Preparation,
        ReleaseRole::Preflight,
        ReleaseRole::PublishBootstrap,
        ReleaseRole::Reconcile,
    ];
    with_bootstrap.sort();
    let preparation = jobs
        .values()
        .find(|job| job.role == ReleaseRole::Preparation);
    let has_writer = jobs.values().any(|job| job.role == ReleaseRole::PrCreate);
    if has_writer {
        required.push(ReleaseRole::PrCreate);
        with_bootstrap.push(ReleaseRole::PrCreate);
        required.sort();
        with_bootstrap.sort();
    }
    if preparation.is_none_or(|job| job.outputs.is_empty() == has_writer)
        || (roles != required && roles != with_bootstrap)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_role_set:{}",
            roles.len()
        )));
    }
    Ok(())
}

fn check_needs_graph(jobs: &BTreeMap<String, ReleaseJobSpec>) -> Result<(), RenderError> {
    for (id, job) in jobs {
        for need in &job.needs {
            let Some(target) = jobs.get(need) else {
                return Err(RenderError::InvalidWorkflow(format!(
                    "unknown_need:{id}:{need}"
                )));
            };
            if need == id {
                return Err(RenderError::InvalidWorkflow(format!("self_need:{id}")));
            }
            if target.role.rank() >= job.role.rank() {
                return Err(RenderError::InvalidWorkflow(format!(
                    "backward_need:{id}:{need}"
                )));
            }
        }
    }
    Ok(())
}

fn check_role_conditions(
    jobs: &BTreeMap<String, ReleaseJobSpec>,
    repository: &str,
    branch: &str,
    plan: &ReleasePlan,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        match job.role {
            ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap => {
                let gate = publish_gate_condition(
                    repository,
                    branch,
                    plan,
                    job.role == ReleaseRole::PublishOidc,
                );
                if job.condition.as_deref() != Some(gate.as_str()) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "publish_gate_mismatch:{id}"
                    )));
                }
            }
            ReleaseRole::Reconcile => {
                let publisher = if jobs
                    .values()
                    .any(|candidate| candidate.role == ReleaseRole::PublishBootstrap)
                {
                    "release-publish-bootstrap"
                } else {
                    "release-publish"
                };
                let condition = format!("always() && needs.{publisher}.result != 'skipped'");
                if job.condition.as_deref() != Some(condition.as_str()) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "reconcile_condition:{id}"
                    )));
                }
            }
            ReleaseRole::Preparation => {
                let expected = format!(
                    "github.repository == '{repository}' && github.event_name == 'push' && github.ref == 'refs/heads/{branch}' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'"
                );
                if job.condition.as_deref() != Some(expected.as_str())
                    || job.needs != ["release-preflight"]
                {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "preparation_gate_mismatch:{id}"
                    )));
                }
            }
            ReleaseRole::PrCreate => {
                let expected = "needs.release-preparation.result == 'success' && needs.release-preparation.outputs.changed == 'true'";
                if job.condition.as_deref() != Some(expected)
                    || job.needs != ["release-preparation"]
                {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "release_pr_create_gate:{id}"
                    )));
                }
            }
            ReleaseRole::Preflight => {}
        }
    }
    Ok(())
}

fn check_publish_needs(jobs: &BTreeMap<String, ReleaseJobSpec>) -> Result<(), RenderError> {
    let id_for = |role: ReleaseRole| {
        jobs.iter()
            .find_map(|(id, job)| (job.role == role).then_some(id))
    };
    let Some(preflight) = id_for(ReleaseRole::Preflight) else {
        return Err(RenderError::InvalidWorkflow(
            "release_role_set:preflight".to_owned(),
        ));
    };
    let publishers: Vec<&String> = jobs
        .iter()
        .filter(|(_, job)| {
            matches!(
                job.role,
                ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap
            )
        })
        .map(|(id, _)| id)
        .collect();
    for publisher in &publishers {
        let gated = jobs
            .get(*publisher)
            .is_some_and(|job| job.needs.contains(preflight));
        if !gated {
            return Err(RenderError::InvalidWorkflow(format!(
                "publish_without_preflight:{publisher}"
            )));
        }
    }
    let Some(reconcile) = id_for(ReleaseRole::Reconcile) else {
        return Err(RenderError::InvalidWorkflow(
            "release_role_set:reconcile".to_owned(),
        ));
    };
    let reconciled = jobs.get(reconcile).is_some_and(|job| {
        publishers
            .iter()
            .all(|publisher| job.needs.contains(*publisher))
    });
    if !reconciled {
        return Err(RenderError::InvalidWorkflow(format!(
            "reconcile_without_publish:{reconcile}"
        )));
    }
    Ok(())
}
