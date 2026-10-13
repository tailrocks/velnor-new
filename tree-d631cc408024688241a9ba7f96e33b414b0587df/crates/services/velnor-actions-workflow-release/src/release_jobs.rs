//! Release roles, job specs, and workflow assembly.
//!
//! The legal role set, the forward-only needs graph, and the exact role
//! conditions. The per-role permission matrix lives in
//! [`crate::release_permissions`].

use std::collections::BTreeMap;

use velnor_actions_contract::validate_job_id;
use velnor_actions_contract_workflow::{JobTimeout, Step, StepKind};

use crate::{
    release_permissions::JobPermissions,
    release_spec::{
        BootstrapPlan, ReleaseConcurrency, ReleaseTriggers, check_lock_anchor, is_clean_text,
        publish_gate_condition, validate_environment, validate_repository,
    },
};
use velnor_actions_workflow_steps::{RenderError, steps::scan_for_private_subcommands};

/// Release boundary role carried by one job (bootstrap optional).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReleaseRole {
    /// Release-PR preparation (GitHub authority, no registry/OIDC).
    Preparation,
    /// Preflight validation (read-only, exact-source checkout).
    Preflight,
    /// Trusted-publishing publication (OIDC, pinned environment).
    PublishOidc,
    /// Bootstrap-token publication (distinct job, pinned environment).
    PublishBootstrap,
    /// Independent reconciliation (read-only, always runs).
    Reconcile,
}

impl ReleaseRole {
    /// Machine-readable role name for errors.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preparation => "preparation",
            Self::Preflight => "preflight",
            Self::PublishOidc => "publish-oidc",
            Self::PublishBootstrap => "publish-bootstrap",
            Self::Reconcile => "reconcile",
        }
    }

    /// True for validation-only roles (never hold write authority).
    #[must_use]
    pub fn is_validation(self) -> bool {
        matches!(self, Self::Preflight | Self::Reconcile)
    }

    /// Forward-only pipeline rank (needs must point to lower ranks).
    fn rank(self) -> u8 {
        match self {
            Self::Preparation => 0,
            Self::Preflight => 1,
            Self::PublishOidc | Self::PublishBootstrap => 2,
            Self::Reconcile => 3,
        }
    }
}

/// One release job: a contract step list plus typed role metadata.
#[derive(Debug, Clone)]
pub struct ReleaseJobSpec {
    /// Boundary role this job serves.
    pub role: ReleaseRole,
    /// Stable display name.
    pub display_name: String,
    /// Literal versioned Ubuntu label.
    pub runs_on: String,
    /// Required per-job bound (G4): release jobs are generated jobs,
    /// so they carry `timeout-minutes` by construction like CI jobs.
    pub timeout_minutes: JobTimeout,
    /// Job dependencies (forward-only by role rank).
    pub needs: Vec<String>,
    /// Run condition (`if`); publish/reconcile roles carry exact gates.
    pub condition: Option<String>,
    /// Pinned GitHub environment (required for publication).
    pub environment: Option<String>,
    /// Least-privilege permissions for the role.
    pub permissions: JobPermissions,
    /// Ordered steps (action/shell only; internal ops rejected).
    pub steps: Vec<Step>,
}

/// Validate one `if:` condition shape (single line, no secret handles).
fn check_condition_text(condition: &str) -> Result<(), RenderError> {
    if !is_clean_text(condition, 512) || condition.contains("secrets.") {
        return Err(RenderError::InvalidWorkflow(format!(
            "bad_condition:{condition}"
        )));
    }
    scan_for_private_subcommands(condition)
}

impl ReleaseJobSpec {
    /// Validate job shape, environment, permissions, and condition text.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for malformed jobs or permission drift.
    pub fn validate_shape(&self, id: &str) -> Result<(), RenderError> {
        if !is_clean_text(&self.display_name, 128) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_job_display:{id}"
            )));
        }
        scan_for_private_subcommands(&self.display_name)?;
        velnor_actions_workflow_tree::guard::validate_runs_on(&self.runs_on)?;
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

/// Complete typed release workflow: identity, triggers, jobs, plan.
#[derive(Debug, Clone)]
pub struct ReleaseWorkflowSpec {
    /// Workflow display name.
    pub name: String,
    /// Exact `owner/repo` identity every gate binds.
    pub repository: String,
    /// Trusted-branch/schedule/typed-dispatch triggers.
    pub triggers: ReleaseTriggers,
    /// Stable serialized concurrency.
    pub concurrency: ReleaseConcurrency,
    /// Jobs keyed by job ID (sorted).
    pub jobs: BTreeMap<String, ReleaseJobSpec>,
    /// Approved exact-source plan.
    pub bootstrap: BootstrapPlan,
    /// Pinned environment for the OIDC publish job.
    pub publish_environment: String,
    /// Pinned environment for the bootstrap publish job.
    pub bootstrap_environment: String,
}

impl ReleaseWorkflowSpec {
    /// Validate identity, triggers, roles, graph, conditions, and binding.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for any structural or binding violation.
    pub fn validate(&self) -> Result<(), RenderError> {
        if !is_clean_text(&self.name, 128) {
            return Err(RenderError::InvalidWorkflow("bad_release_name".to_owned()));
        }
        scan_for_private_subcommands(&self.name)?;
        validate_repository(&self.repository)?;
        self.bootstrap.validate()?;
        if self.bootstrap.repository != self.repository {
            return Err(RenderError::InvalidWorkflow(
                "bootstrap_repository_mismatch".to_owned(),
            ));
        }
        validate_environment(&self.publish_environment)?;
        validate_environment(&self.bootstrap_environment)?;
        self.triggers.validate(&self.bootstrap)?;
        self.concurrency.validate()?;
        check_lock_anchor(&self.concurrency.group, &self.repository)?;
        check_role_set(&self.jobs)?;
        for (id, job) in &self.jobs {
            job.validate_shape(id)?;
        }
        check_needs_graph(&self.jobs)?;
        check_role_conditions(&self.jobs, &self.repository, &self.bootstrap)?;
        check_publish_needs(&self.jobs)?;
        Ok(())
    }
}

/// Require valid IDs plus the exact role set (bootstrap optional).
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
    let mut with_bootstrap = required.clone();
    with_bootstrap.push(ReleaseRole::PublishBootstrap);
    with_bootstrap.sort();
    if roles != required && roles != with_bootstrap {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_role_set:{}",
            roles.len()
        )));
    }
    Ok(())
}

/// Require known, forward-only needs (rank order forbids cycles).
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

/// Require exact publish gates plus the always-on reconcile condition.
fn check_role_conditions(
    jobs: &BTreeMap<String, ReleaseJobSpec>,
    repository: &str,
    bootstrap: &BootstrapPlan,
) -> Result<(), RenderError> {
    let gate = publish_gate_condition(repository, bootstrap);
    for (id, job) in jobs {
        match job.role {
            ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap => {
                if job.condition.as_deref() != Some(gate.as_str()) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "publish_gate_mismatch:{id}"
                    )));
                }
            }
            ReleaseRole::Reconcile => {
                if job.condition.as_deref() != Some("always()") {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "reconcile_condition:{id}"
                    )));
                }
            }
            ReleaseRole::Preparation | ReleaseRole::Preflight => {}
        }
    }
    Ok(())
}

/// Require publication after preflight and reconciliation after publishing.
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
