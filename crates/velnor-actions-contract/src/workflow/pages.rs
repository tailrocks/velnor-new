//! Closed Pages deployment authority; compiled owners supply approved bindings.
use super::source_helper::SourceBoundOperation::{
    MiseBootstrap, MiseToolPrepare, NativePagesToolPreparation,
};
use super::{
    ir::{Job, WorkflowIr},
    outputs::ActionOutput,
    permissions::{PermissionLevel, Permissions},
    source_helper::HelperInvocation,
    step::{StepId, StepKind},
};
use crate::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Exact action approvals supplied by the compiled orchestration factory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePagesActions {
    /// Approved source checkout action.
    pub checkout: String,
    /// Approved Pages configuration action.
    pub configure: String,
    /// Approved Pages archive upload action.
    pub upload: String,
    /// Approved OIDC deployment action.
    pub deploy: String,
}

/// Closed event gate for protected native Pages publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativePagesTriggerPolicy {
    /// Protected source pushes and explicitly requested dispatches.
    ProtectedPushDispatch,
    /// Scheduled publication or dispatch selecting the literal publish mode.
    ScheduledExplicitPublish,
}

/// Exact compiled trusted tool preparation; never repository computation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePagesPreparation {
    /// Mandatory unconditional preparation step.
    pub step_id: StepId,
    /// Exact compiled operation and selectors.
    pub invocation: HelperInvocation,
    /// Exact environment approved by the compiled owner.
    pub environment: BTreeMap<String, String>,
}

/// Deployment receives narrow OIDC only after source and artifact admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePagesDeploy {
    /// Exact approved repository identity.
    pub repository: String,
    /// Protected default source branch.
    pub default_branch: String,
    /// Closed reviewed event gate.
    pub trigger_policy: NativePagesTriggerPolicy,
    /// Exact approved executable setup before admission.
    pub preparation: Vec<NativePagesPreparation>,
    /// Mandatory successful full CI dependency.
    pub full_ci_job: String,
    /// Exact compiled full CI proof invocation, checked in its dependency.
    pub full_ci_admission: HelperInvocation,
    /// Immutable verified artifact producer dependency.
    pub artifact_job: String,
    /// Producer's typed immutable artifact identifier output.
    pub artifact_id_output: String,
    /// Producer's typed immutable digest output.
    pub artifact_digest_output: String,
    /// Mandatory closed source/artifact/branch admission step.
    pub admission_step: StepId,
    /// Exact compiled admission invocation.
    pub admission: HelperInvocation,
    /// Exact additional environment from the compiled native execution owner.
    pub admission_extra_environment: BTreeMap<String, String>,
    /// Mandatory deployment step.
    pub deploy_step: StepId,
    /// Approved action identities from the authoritative factory.
    pub actions: NativePagesActions,
}

impl NativePagesDeploy {
    /// Closed protected source condition; dispatch retains every source gate.
    #[must_use]
    pub fn condition(&self) -> String {
        let events = match self.trigger_policy {
            NativePagesTriggerPolicy::ProtectedPushDispatch => {
                "(github.event_name == 'push' || github.event_name == 'workflow_dispatch')"
            }
            NativePagesTriggerPolicy::ScheduledExplicitPublish => {
                "(github.event_name == 'schedule' || (github.event_name == 'workflow_dispatch' && inputs.mode == 'publish'))"
            }
        };
        format!(
            "success() && github.repository == '{}' && github.ref == 'refs/heads/{}' && {events}",
            self.repository, self.default_branch
        )
    }

    /// Exact source and immutable artifact authority for the compiled guard.
    #[must_use]
    pub fn admission_environment(&self) -> BTreeMap<String, String> {
        let mut environment = self.admission_extra_environment.clone();
        environment.extend(BTreeMap::from([
            ("APPROVED_REPOSITORY".to_owned(), self.repository.clone()),
            (
                "APPROVED_DEFAULT_BRANCH".to_owned(),
                self.default_branch.clone(),
            ),
            (
                "APPROVED_SOURCE_SHA".to_owned(),
                "${{ github.sha }}".to_owned(),
            ),
            (
                "FULL_CI_RESULT".to_owned(),
                format!("${{{{ needs.{}.result }}}}", self.full_ci_job),
            ),
            (
                "EXPECTED_ARTIFACT_ID".to_owned(),
                format!(
                    "${{{{ needs.{}.outputs.{} }}}}",
                    self.artifact_job, self.artifact_id_output
                ),
            ),
            (
                "EXPECTED_ARTIFACT_DIGEST".to_owned(),
                format!(
                    "${{{{ needs.{}.outputs.{} }}}}",
                    self.artifact_job, self.artifact_digest_output
                ),
            ),
        ]));
        environment.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
        environment
    }

    /// Validate a deployment against its workflow graph and closed role.
    /// # Errors
    /// Rejects wider permission grants, conditional bypasses, or foreign bindings.
    pub fn validate(&self, job: &Job, workflow: &WorkflowIr) -> Result<(), ContractError> {
        self.validate_identity()?;
        let expected = Permissions {
            contents: PermissionLevel::Read,
            actions: PermissionLevel::Read,
            id_token: PermissionLevel::Write,
            pages: PermissionLevel::Write,
            attestations: PermissionLevel::None,
            pull_requests: PermissionLevel::None,
            issues: PermissionLevel::None,
        };
        if job.permissions.as_ref() != Some(&expected)
            || job.environment.as_deref() != Some("github-pages")
            || job.condition.as_deref() != Some(self.condition().as_str())
            || job.native_publish.is_some()
            || job.source_producer.is_some()
            || job.tool_producer.is_some()
            || job.steps.iter().any(|step| step.condition.is_some())
            || !job.needs.contains(&self.full_ci_job)
            || !job.needs.contains(&self.artifact_job)
            || !workflow.jobs.contains_key(&self.full_ci_job)
        {
            return Err(invalid("invalid_deployment_authority"));
        }
        self.validate_triggers(workflow)?;
        self.validate_full_ci(workflow)?;
        self.validate_artifact(workflow)?;
        self.validate_steps(job)?;
        Ok(())
    }

    fn validate_identity(&self) -> Result<(), ContractError> {
        let repository = self.repository.split('/').collect::<Vec<_>>();
        if repository.len() != 2
            || repository.iter().any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            })
            || !crate::is_valid_branch_name(&self.default_branch)
            || self.default_branch.contains('\'')
        {
            return Err(invalid("invalid_protected_source"));
        }
        for name in [
            &self.full_ci_job,
            &self.artifact_job,
            &self.artifact_id_output,
            &self.artifact_digest_output,
        ] {
            StepId::new(name)?;
        }
        self.admission_step.validate()?;
        self.deploy_step.validate()?;
        self.admission.validate()?;
        self.full_ci_admission.validate()?;
        if self.admission_step == self.deploy_step
            || self.admission.descriptor().operation()
                != super::source_helper::SourceBoundOperation::NativePagesAdmission
            || self.full_ci_admission.descriptor().operation()
                != super::source_helper::SourceBoundOperation::ReleaseAdmissionDefaultBranch
        {
            return Err(invalid("invalid_admission_binding"));
        }
        let required = self.admission_environment();
        if self.admission_extra_environment.iter().any(|(key, value)| {
            required.get(key) != Some(value)
                || value.contains("secrets.")
                || (key != "GH_TOKEN" && value.contains("github.token"))
        }) {
            return Err(invalid("foreign_admission_environment"));
        }
        let operations = self
            .preparation
            .iter()
            .map(|step| step.invocation.descriptor().operation())
            .collect::<Vec<_>>();
        if !matches!(
            operations.as_slice(),
            [] | [MiseToolPrepare | NativePagesToolPreparation]
                | [MiseBootstrap, MiseToolPrepare | NativePagesToolPreparation]
        ) {
            return Err(invalid("invalid_preparation_order"));
        }
        self.actions.validate()
    }

    fn validate_artifact(&self, workflow: &WorkflowIr) -> Result<(), ContractError> {
        let producer = workflow
            .jobs
            .get(&self.artifact_job)
            .ok_or_else(|| invalid("missing_artifact_producer"))?;
        let id = producer
            .outputs
            .iter()
            .find(|output| output.name == self.artifact_id_output);
        let digest = producer
            .outputs
            .iter()
            .find(|output| output.name == self.artifact_digest_output);
        if !id.zip(digest).is_some_and(|(id, digest)| {
            id.value.output == ActionOutput::ArtifactId
                && digest.value.output == ActionOutput::ArtifactDigest
                && id.value.step_id == digest.value.step_id
        }) {
            return Err(invalid("invalid_immutable_artifact_binding"));
        }
        super::outputs::validate_job_outputs(&producer.outputs, &producer.steps)
    }
}

impl NativePagesActions {
    fn validate(&self) -> Result<(), ContractError> {
        for (uses, owner) in [
            (&self.checkout, "actions/checkout"),
            (&self.configure, "actions/configure-pages"),
            (&self.upload, "actions/upload-pages-artifact"),
            (&self.deploy, "actions/deploy-pages"),
        ] {
            if !uses.split_once('@').is_some_and(|(action, sha)| {
                action == owner && crate::ids::is_lower_hex_len(sha, 40)
            }) {
                return Err(invalid("invalid_action_approval"));
            }
        }
        Ok(())
    }
}

fn invalid(problem: &str) -> ContractError {
    ContractError::identity("native_pages_deploy", problem)
}

#[path = "pages_steps.rs"]
mod steps;

impl NativePagesDeploy {
    fn validate_full_ci(&self, workflow: &WorkflowIr) -> Result<(), ContractError> {
        let proof = workflow
            .jobs
            .get(&self.full_ci_job)
            .ok_or_else(|| invalid("missing_full_ci_proof"))?;
        let exact = proof
            .steps
            .iter()
            .filter(|step| {
                matches!(&step.kind,
            StepKind::SourceBoundHelper { invocation, .. } if invocation == &self.full_ci_admission)
            })
            .collect::<Vec<_>>();
        if self.full_ci_job == self.artifact_job
            || exact.len() != 1
            || exact[0].condition.is_some()
            || proof.native_pages_deploy.is_some()
        {
            return Err(invalid("invalid_full_ci_proof"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "pages_tests.rs"]
mod tests;

impl NativePagesDeploy {
    fn validate_triggers(&self, workflow: &WorkflowIr) -> Result<(), ContractError> {
        let triggers = &workflow.triggers;
        match self.trigger_policy {
            NativePagesTriggerPolicy::ProtectedPushDispatch => {
                if !triggers.push_branches.contains(&self.default_branch)
                    && triggers.workflow_dispatch.is_none()
                {
                    return Err(invalid("missing_protected_source_event"));
                }
            }
            NativePagesTriggerPolicy::ScheduledExplicitPublish => {
                if triggers.schedule.is_none() && triggers.workflow_dispatch.is_none() {
                    return Err(invalid("missing_scheduled_publish_event"));
                }
                if let Some(dispatch) = &triggers.workflow_dispatch {
                    let mode = dispatch.inputs.iter().find(|input| input.name == "mode");
                    if !mode.is_some_and(|input| {
                        input.input_type == super::dispatch::DispatchInputType::Choice
                            && input.options.iter().any(|option| option == "publish")
                    }) {
                        return Err(invalid("missing_explicit_publish_input"));
                    }
                }
            }
        }
        Ok(())
    }
}
