//! Release roles, job specs, and workflow assembly.
//!
//! The legal role set, the forward-only needs graph, and the exact role
//! conditions. The per-role permission matrix lives in
//! [`crate::release_permissions`].

use std::collections::BTreeMap;

use velnor_actions_contract::{JobTimeout, Step, StepKind};

use crate::{
    RenderError,
    release_permissions::JobPermissions,
    release_spec::{
        BootstrapPlan, ReleaseConcurrency, ReleaseTriggers, check_lock_anchor, is_clean_text,
        validate_environment, validate_repository,
    },
    steps::scan_for_private_subcommands,
};

/// Closed authority boundaries in the immutable release pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReleaseRole {
    /// Fresh read-only source API snapshot producer with no repository execution.
    SourceSnapshotForge,
    /// Anonymous Cargo packaging with no ancestors.
    PackageAnonymous,
    /// Fresh anonymous preparation of original archives from immutable source.
    PackagePreparedAnonymous,
    /// Fresh read-only immutable package admission.
    PreflightForge,
    /// Token-only crates.io publication using OIDC.
    RegistryPublishOidc,
    /// Token-only first publication using the approved bootstrap secret.
    RegistryPublishBootstrap,
    /// Fresh write-only GitHub tag and release coordination.
    ForgePublish,
    /// Fresh independent publication reconciliation.
    Reconcile,
    /// Anonymous proposed manifest/lock/changelog generation.
    PreparationAnonymous,
    /// Fresh GitHub `GitData` and pull request coordination.
    PreparationForge,
}

impl ReleaseRole {
    /// Fixed role spelling for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceSnapshotForge => "source-snapshot-forge",
            Self::PackageAnonymous => "package-anonymous",
            Self::PackagePreparedAnonymous => "package-prepared-anonymous",
            Self::PreflightForge => "preflight-forge",
            Self::RegistryPublishOidc => "registry-publish-oidc",
            Self::RegistryPublishBootstrap => "registry-publish-bootstrap",
            Self::ForgePublish => "forge-publish",
            Self::Reconcile => "reconcile",
            Self::PreparationAnonymous => "preparation-anonymous",
            Self::PreparationForge => "preparation-forge",
        }
    }

    /// Exact producer identity also used by authenticated artifact transport.
    #[must_use]
    pub const fn job_id(self) -> &'static str {
        match self {
            Self::SourceSnapshotForge => "release-source-snapshot",
            Self::PackageAnonymous | Self::PackagePreparedAnonymous => "release-package",
            Self::PreflightForge => "release-preflight",
            Self::RegistryPublishOidc | Self::RegistryPublishBootstrap => {
                "release-registry-publish"
            }
            Self::ForgePublish => "release-forge-publish",
            Self::Reconcile => "release-reconcile",
            Self::PreparationAnonymous => "release-preparation-source",
            Self::PreparationForge => "release-preparation",
        }
    }

    /// Roles which never possess write credentials.
    #[must_use]
    pub const fn is_validation(self) -> bool {
        matches!(
            self,
            Self::SourceSnapshotForge
                | Self::PackageAnonymous
                | Self::PackagePreparedAnonymous
                | Self::PreflightForge
                | Self::Reconcile
                | Self::PreparationAnonymous
        )
    }

    /// Forward-only role order.
    pub(super) const fn rank(self) -> u8 {
        match self {
            Self::SourceSnapshotForge => 0,
            Self::PackageAnonymous
            | Self::PackagePreparedAnonymous
            | Self::PreparationAnonymous => 1,
            Self::PreflightForge | Self::PreparationForge => 2,
            Self::RegistryPublishOidc | Self::RegistryPublishBootstrap => 3,
            Self::ForgePublish => 4,
            Self::Reconcile => 5,
        }
    }
}

/// One release job: a contract step list plus typed role metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Exact declared helper and artifact outputs, reconstructed by the owner.
    pub outputs: Vec<velnor_actions_contract::workflow::outputs::JobOutput>,
}

/// Validate one `if:` condition shape (single line, no secret handles).
fn check_condition_text(condition: &str) -> Result<(), RenderError> {
    if !is_clean_text(condition, 1024) || condition.contains("secrets.") {
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
        crate::guard::validate_runs_on(&self.runs_on)?;
        if self.steps.is_empty() {
            return Err(RenderError::InvalidWorkflow(format!("empty_steps:{id}")));
        }
        velnor_actions_contract::workflow::step::validate_step_ids(&self.steps)
            .map_err(RenderError::Contract)?;
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Whether the approved release policy requires proposal preparation.
    pub preparation_enabled: bool,
    /// Exact frozen execution records supplied by each compiled helper owner.
    pub helper_registry: Vec<velnor_actions_contract::CompiledSourceHelper>,
    /// Complete marked support bytes supplied by their compiled source owner.
    pub support_sources: Vec<velnor_actions_contract::CompiledSupportSource>,
    /// Frozen checkout and source-owner bootstrap approval.
    pub bootstrap_tools: crate::release_bootstrap::ReleaseBootstrapApproval,
    /// Approved exact-source plan.
    pub bootstrap: BootstrapPlan,
    /// Immutable package, ownership, tool, and authentication approval.
    pub reconciliation: crate::release_spec::ReleaseReconcilePolicy,
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
        self.bootstrap_tools.validate()?;
        self.bootstrap.validate()?;
        self.reconciliation.validate(&self.bootstrap)?;
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
        check_role_set(
            &self.jobs,
            self.bootstrap.version.is_some(),
            self.preparation_enabled,
        )?;
        for (id, job) in &self.jobs {
            job.validate_shape(id)?;
            self.bootstrap_tools
                .check_registry(&self.helper_registry, &job.runs_on)?;
        }
        check_needs_graph(&self.jobs)?;
        check_role_conditions(
            &self.jobs,
            &self.repository,
            &self.bootstrap,
            &self.triggers.push_branches,
        )?;
        self.check_publisher_environments()?;
        check_publish_needs(&self.jobs)?;

        Ok(())
    }

    /// Publisher authority binds the workflow's exact protected environments.
    fn check_publisher_environments(&self) -> Result<(), RenderError> {
        for (id, job) in &self.jobs {
            let expected = match job.role {
                ReleaseRole::RegistryPublishOidc | ReleaseRole::PreparationForge => {
                    &self.publish_environment
                }
                ReleaseRole::RegistryPublishBootstrap => &self.bootstrap_environment,
                ReleaseRole::ForgePublish if self.bootstrap.version.is_some() => {
                    &self.bootstrap_environment
                }
                ReleaseRole::ForgePublish => &self.publish_environment,
                _ => {
                    if job.environment.is_some() {
                        return Err(RenderError::InvalidWorkflow(format!(
                            "release_validation_environment:{id}"
                        )));
                    }
                    continue;
                }
            };
            if job.environment.as_ref() != Some(expected) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "publish_environment_mismatch:{id}"
                )));
            }
        }
        Ok(())
    }
}

#[path = "release_graph_gates.rs"]
mod graph;
use graph::{check_needs_graph, check_publish_needs, check_role_conditions, check_role_set};
