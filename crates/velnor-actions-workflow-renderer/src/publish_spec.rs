//! Publication workflow IR: roles, permissions, jobs, triggers and validation.
//!
//! Publication runs only from a `push` to the default branch or a release
//! tag (hard decision 8). The IR therefore has no pull-request,
//! `pull_request_target`, `workflow_run` or dispatch trigger, and every job
//! except the authority job must transitively need that authority job.
//! Privileged roles never check out repository code.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{JobTimeout, Step, StepKind, is_valid_branch_name, validate_job_id};

use crate::{
    RenderError,
    release_permissions::PermissionLevel,
    release_spec::{is_clean_text, validate_environment, validate_repository},
    steps::scan_for_private_subcommands,
};

/// Publication boundary role carried by one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PublishRole {
    /// Verifies tag release authority; reads GitHub metadata only, no checkout.
    Authority,
    /// Verifies default-branch push authority; waits for `Required`, no checkout.
    BranchAuthority,
    /// Creates tags and GitHub releases from the verified default-branch commit.
    TagRelease,
    /// Builds an artifact from the checked-out tag commit; read-only.
    ArtifactBuild,
    /// Attests the built artifact; holds OIDC and attestation authority, no checkout.
    ArtifactAttest,
    /// Uploads the attested artifact as a release asset; no checkout.
    ArtifactPublish,
}

impl PublishRole {
    /// Machine-readable role name for errors.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authority => "authority",
            Self::BranchAuthority => "branch-authority",
            Self::TagRelease => "tag-release",
            Self::ArtifactBuild => "artifact-build",
            Self::ArtifactAttest => "artifact-attest",
            Self::ArtifactPublish => "artifact-publish",
        }
    }

    /// Forward-only pipeline rank (needs must point to lower ranks).
    fn rank(self) -> u8 {
        match self {
            Self::Authority | Self::BranchAuthority => 0,
            Self::TagRelease | Self::ArtifactBuild => 1,
            Self::ArtifactAttest => 2,
            Self::ArtifactPublish => 3,
        }
    }

    /// True for roles that execute no repository code and never check it out.
    ///
    /// `ArtifactBuild` only reads files; `TagRelease` checks out so the pinned
    /// release tool can read manifests and history, and runs no repository code.
    fn forbids_checkout(self) -> bool {
        !matches!(self, Self::ArtifactBuild | Self::TagRelease)
    }

    /// True for the roles that gate the rest of the workflow.
    fn is_authority(self) -> bool {
        matches!(self, Self::Authority | Self::BranchAuthority)
    }

    /// The authority role a downstream role must transitively need.
    fn gate(self) -> Option<Self> {
        match self {
            Self::Authority | Self::BranchAuthority => None,
            Self::TagRelease => Some(Self::BranchAuthority),
            Self::ArtifactBuild | Self::ArtifactAttest | Self::ArtifactPublish => {
                Some(Self::Authority)
            }
        }
    }
}

/// Per-job least-privilege permissions (typed levels only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishPermissions {
    /// Repository contents access.
    pub contents: PermissionLevel,
    /// Check-run read access (the `Required` verdict).
    pub checks: PermissionLevel,
    /// OIDC token access (attestation signing).
    pub id_token: PermissionLevel,
    /// Artifact attestation access.
    pub attestations: PermissionLevel,
}

impl PublishPermissions {
    /// Exact least-privilege matrix for one role.
    #[must_use]
    pub fn expected(role: PublishRole) -> Self {
        use PermissionLevel::{None, Read, Write};
        let (contents, checks, id_token, attestations) = match role {
            PublishRole::Authority | PublishRole::BranchAuthority => (Read, Read, None, None),
            PublishRole::ArtifactBuild => (Read, None, None, None),
            PublishRole::ArtifactAttest => (Read, None, Write, Write),
            PublishRole::TagRelease | PublishRole::ArtifactPublish => (Write, None, None, None),
        };
        Self {
            contents,
            checks,
            id_token,
            attestations,
        }
    }
}

/// One step plus the optional GitHub step id its outputs are read through.
#[derive(Debug, Clone)]
pub struct PublishStep {
    /// Step id (`[a-z][a-z0-9-]*`); required when a job output reads it.
    pub id: Option<String>,
    /// The validated action or shell step.
    pub step: Step,
}

/// One publication job.
#[derive(Debug, Clone)]
pub struct PublishJobSpec {
    /// Boundary role this job serves.
    pub role: PublishRole,
    /// Stable display name.
    pub display_name: String,
    /// Literal versioned runner label.
    pub runs_on: String,
    /// Required per-job bound.
    pub timeout_minutes: JobTimeout,
    /// Job dependencies (forward-only by role rank).
    pub needs: Vec<String>,
    /// Run condition (`if`), single line without secret handles.
    pub condition: Option<String>,
    /// Pinned GitHub environment (required for asset upload).
    pub environment: Option<String>,
    /// Job outputs: name to `${{ steps.<id>.outputs.<name> }}`.
    pub outputs: BTreeMap<String, String>,
    /// Permissions; must equal the role matrix.
    pub permissions: PublishPermissions,
    /// Ordered steps.
    pub steps: Vec<PublishStep>,
}

/// Trusted publication triggers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishTriggers {
    /// Exact default-branch names.
    pub branches: Vec<String>,
    /// Tag glob admitting release tags, if tag-driven kinds exist.
    pub tag_glob: Option<String>,
}

/// Complete publication workflow.
#[derive(Debug, Clone)]
pub struct PublishWorkflowSpec {
    /// Workflow display name.
    pub name: String,
    /// Exact `owner/repo` identity.
    pub repository: String,
    /// Push triggers.
    pub triggers: PublishTriggers,
    /// Jobs keyed by job id.
    pub jobs: BTreeMap<String, PublishJobSpec>,
}

impl PublishJobSpec {
    /// Validate job shape, role matrix and step content.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for malformed jobs or permission drift.
    pub fn validate_shape(&self, id: &str) -> Result<(), RenderError> {
        let bad = |what: &str| RenderError::InvalidWorkflow(format!("{what}:{id}"));
        if !is_clean_text(&self.display_name, 128) {
            return Err(bad("bad_job_display"));
        }
        scan_for_private_subcommands(&self.display_name)?;
        crate::guard::validate_runs_on(&self.runs_on)?;
        if self.steps.is_empty() {
            return Err(bad("empty_steps"));
        }
        if self.permissions != PublishPermissions::expected(self.role) {
            return Err(bad("permission_matrix"));
        }
        let writes = self.permissions.contents == PermissionLevel::Write;
        if writes != self.environment.is_some() {
            return Err(bad("environment_binding"));
        }
        if let Some(environment) = &self.environment {
            validate_environment(environment)?;
        }
        if let Some(condition) = &self.condition {
            if !is_clean_text(condition, 512) || condition.contains("secrets.") {
                return Err(bad("bad_condition"));
            }
            scan_for_private_subcommands(condition)?;
        }
        for step in &self.steps {
            self.validate_step(id, step)?;
        }
        self.validate_outputs(id)
    }

    /// Validate one step: clean name, no internal ops, role checkout rule.
    fn validate_step(&self, id: &str, step: &PublishStep) -> Result<(), RenderError> {
        let bad = |what: &str| RenderError::InvalidWorkflow(format!("{what}:{id}"));
        if !is_clean_text(&step.step.name, 128) {
            return Err(bad("bad_step_name"));
        }
        scan_for_private_subcommands(&step.step.name)?;
        if let Some(step_id) = &step.id
            && !is_step_id(step_id)
        {
            return Err(bad("bad_step_id"));
        }
        match &step.step.kind {
            StepKind::Internal { .. } | StepKind::TaskExecution { .. } => {
                return Err(bad("publish_internal_op"));
            }
            StepKind::Action { uses, .. }
                if self.role.forbids_checkout() && uses.starts_with("actions/checkout@") =>
            {
                return Err(bad("checkout_in_privileged_role"));
            }
            _ => {}
        }
        Ok(())
    }

    /// Validate job outputs: each reads a declared step id's output.
    fn validate_outputs(&self, id: &str) -> Result<(), RenderError> {
        let bad = || RenderError::InvalidWorkflow(format!("bad_job_output:{id}"));
        for (name, value) in &self.outputs {
            let slug = |s: &str| {
                !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            };
            let inner = value
                .strip_prefix("${{ steps.")
                .and_then(|rest| rest.strip_suffix(" }}"))
                .ok_or_else(bad)?;
            let (step_id, output) = inner.split_once(".outputs.").ok_or_else(bad)?;
            let declared = self.steps.iter().any(|s| s.id.as_deref() == Some(step_id));
            if !slug(name) || !slug(output) || !declared {
                return Err(bad());
            }
        }
        Ok(())
    }
}

/// Step id shape: lowercase word characters and hyphens, letter first.
fn is_step_id(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl PublishWorkflowSpec {
    /// Validate identity, triggers, role set, needs graph and conditions.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for any structural violation.
    pub fn validate(&self) -> Result<(), RenderError> {
        if !is_clean_text(&self.name, 128) {
            return Err(RenderError::InvalidWorkflow("bad_publish_name".to_owned()));
        }
        scan_for_private_subcommands(&self.name)?;
        validate_repository(&self.repository)?;
        self.validate_triggers()?;
        let mut authorities = BTreeMap::new();
        for (id, job) in &self.jobs {
            validate_job_id(id).map_err(RenderError::Contract)?;
            job.validate_shape(id)?;
            if job.role.is_authority() && authorities.insert(job.role, id.as_str()).is_some() {
                return Err(RenderError::InvalidWorkflow(
                    "publish_authority_count".to_owned(),
                ));
            }
        }
        if authorities.is_empty() {
            return Err(RenderError::InvalidWorkflow(
                "publish_authority_count".to_owned(),
            ));
        }
        self.check_needs(&authorities)
    }

    /// Validate branches and the tag glob.
    fn validate_triggers(&self) -> Result<(), RenderError> {
        let triggers = &self.triggers;
        if triggers.branches.is_empty() && triggers.tag_glob.is_none() {
            return Err(RenderError::InvalidWorkflow(
                "no_publish_trigger".to_owned(),
            ));
        }
        let unique: BTreeSet<&String> = triggers.branches.iter().collect();
        if triggers.branches.len() > 8 || unique.len() != triggers.branches.len() {
            return Err(RenderError::InvalidWorkflow(
                "bad_publish_branches".to_owned(),
            ));
        }
        for branch in &triggers.branches {
            if !is_valid_branch_name(branch) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "bad_push_branch:{branch}"
                )));
            }
            scan_for_private_subcommands(branch)?;
        }
        if let Some(glob) = &triggers.tag_glob {
            let plain =
                |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'*' | b'[' | b']' | b'.');
            if !is_clean_text(glob, 128) || !glob.bytes().all(plain) {
                return Err(RenderError::InvalidWorkflow(format!("bad_tag_glob:{glob}")));
            }
        }
        Ok(())
    }

    /// Require known forward-only needs, then the authority gate on every job.
    fn check_needs(&self, authorities: &BTreeMap<PublishRole, &str>) -> Result<(), RenderError> {
        for (id, job) in &self.jobs {
            if job.role.is_authority() {
                if !job.needs.is_empty() {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "authority_shape:{id}"
                    )));
                }
                continue;
            }
            for need in &job.needs {
                let Some(target) = self.jobs.get(need) else {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "unknown_need:{id}:{need}"
                    )));
                };
                if target.role.rank() >= job.role.rank() {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "backward_need:{id}:{need}"
                    )));
                }
            }
        }
        for (id, job) in &self.jobs {
            let Some(gate) = job.role.gate() else {
                continue;
            };
            let gated = authorities
                .get(&gate)
                .is_some_and(|authority| self.reaches(id, authority));
            if !gated {
                return Err(RenderError::InvalidWorkflow(format!("ungated_job:{id}")));
            }
        }
        Ok(())
    }

    /// True when `from` transitively needs `target` (acyclic by rank order).
    fn reaches(&self, from: &str, target: &str) -> bool {
        let mut stack = vec![from];
        let mut seen = BTreeSet::new();
        while let Some(current) = stack.pop() {
            if current == target {
                return true;
            }
            if seen.insert(current)
                && let Some(job) = self.jobs.get(current)
            {
                stack.extend(job.needs.iter().map(String::as_str));
            }
        }
        false
    }
}
