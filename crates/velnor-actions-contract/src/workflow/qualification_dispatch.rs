//! Source-bound authorization for hosted qualification dispatches.

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::workflow::jobs::CI_WORKFLOW_PATH;
use crate::workflow::plan::{ObligationDecision, Plan, WorkflowEvent};
use crate::workflow::qualification_phase::QualificationPhase;

/// Validate that a serialized plan carries qualification context only when its
/// source, run, trust, baseline, and selected work still satisfy the dispatch
/// contract.
pub(super) fn validate_plan_qualification(plan: &Plan) -> Result<(), ContractError> {
    match (plan.event, plan.qualification.as_ref()) {
        (WorkflowEvent::Qualification, Some(context)) => {
            context.validate_shape()?;
            if context.source_sha != plan.head
                || plan.run_key
                    != crate::ids::run_key_for_ci(context.run_id, u64::from(context.run_attempt))
                || plan.baseline.status() != crate::workflow::baseline::BaselineStatus::Unavailable
            {
                return Err(ContractError::identity(
                    "qualification.binding",
                    "source_run_or_baseline_mismatch",
                ));
            }
            if plan.trust != crate::workflow::trust::Trust::Pr {
                return Err(ContractError::identity(
                    "trust",
                    "qualification_must_remain_untrusted",
                ));
            }
            if plan
                .obligations
                .iter()
                .any(|obligation| obligation.decision != ObligationDecision::Execute)
            {
                return Err(ContractError::identity(
                    "qualification.obligations",
                    "must_execute_all",
                ));
            }
        }
        (WorkflowEvent::Qualification, None) => {
            return Err(ContractError::identity(
                "qualification",
                "missing_dispatch_context",
            ));
        }
        (_, Some(_)) => {
            return Err(ContractError::identity(
                "qualification",
                "context_on_non_qualification_event",
            ));
        }
        (_, None) => {}
    }
    Ok(())
}

/// Immutable runner context captured for one qualification dispatch.
///
/// These fields are evidence to compare at planning and merge admission;
/// they do not grant general push trust or authorize unrelated writers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationDispatch {
    /// Isolated experiment namespace, supplied by the dispatcher.
    pub campaign: String,
    /// Cold, warm, or cache-disabled control phase.
    pub phase: QualificationPhase,
    /// Actual `GITHUB_REPOSITORY` value.
    pub repository: String,
    /// Repository default branch from the immutable dispatch payload.
    pub default_branch: String,
    /// Actual `GITHUB_REF` value.
    pub git_ref: String,
    /// Actual `GITHUB_REF_PROTECTED` value.
    pub ref_protected: bool,
    /// Actual `GITHUB_WORKFLOW_REF` value.
    pub workflow_ref: String,
    /// Actual `GITHUB_WORKFLOW_SHA` value.
    pub workflow_sha: String,
    /// Actual `GITHUB_SHA` value and candidate source revision.
    pub source_sha: String,
    /// Actual `GITHUB_RUN_ID`, retained for attempt binding.
    pub run_id: u64,
    /// Actual `GITHUB_RUN_ATTEMPT`, retained for attempt binding.
    pub run_attempt: u32,
    /// Run and attempt that produced the required predecessor receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predecessor: Option<QualificationRunRef>,
}

/// Lookup coordinates for a previous run; they grant no cache access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRunRef {
    /// GitHub Actions run ID.
    pub run_id: u64,
    /// Exact run attempt whose receipt must be admitted.
    pub run_attempt: u32,
}

impl QualificationRunRef {
    /// Validate nonzero run and attempt coordinates.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.run_id == 0 || self.run_attempt == 0 {
            return Err(ContractError::identity(
                "qualification.predecessor",
                "invalid_run_reference",
            ));
        }
        let run_key = crate::ids::run_key_for_ci(self.run_id, u64::from(self.run_attempt));
        crate::ids::validate_run_key(&run_key)?;
        crate::ids::plan_id_for_run(&run_key)?;
        Ok(())
    }
}

impl QualificationDispatch {
    /// Validate runner provenance against the configured default branch and
    /// the repository and source revision selected by the plan request.
    ///
    /// # Errors
    pub fn validate_for(
        &self,
        default_branch: &str,
        expected_repository: &str,
        expected_source_sha: &str,
    ) -> Result<(), ContractError> {
        self.validate_shape()?;
        if self.repository != expected_repository {
            return Err(ContractError::identity(
                "qualification.repository",
                "repository_mismatch",
            ));
        }
        if self.default_branch != default_branch
            || self.git_ref != format!("refs/heads/{default_branch}")
        {
            return Err(ContractError::identity(
                "qualification.ref",
                "not_default_branch",
            ));
        }
        if !self.ref_protected {
            return Err(ContractError::identity(
                "qualification.ref_protected",
                "unprotected_ref",
            ));
        }
        let expected_workflow_ref = format!(
            "{}/{CI_WORKFLOW_PATH}@refs/heads/{default_branch}",
            self.repository
        );
        if self.workflow_ref != expected_workflow_ref {
            return Err(ContractError::identity(
                "qualification.workflow_ref",
                "workflow_ref_mismatch",
            ));
        }
        if self.source_sha != expected_source_sha
            || self.workflow_sha != expected_source_sha
            || self.source_sha != self.workflow_sha
        {
            return Err(ContractError::identity(
                "qualification.source_sha",
                "source_or_workflow_sha_mismatch",
            ));
        }
        Ok(())
    }

    /// Validate canonical fields without relying on external configuration.
    ///
    /// # Errors
    pub fn validate_shape(&self) -> Result<(), ContractError> {
        validate_campaign(&self.campaign)?;
        validate_repository(&self.repository)?;
        validate_source_binding(self)?;
        validate_ref_binding(self)?;
        validate_run_identity(self)?;
        validate_predecessor(self)
    }
}

fn validate_predecessor(context: &QualificationDispatch) -> Result<(), ContractError> {
    match (context.phase.predecessor(), context.predecessor) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(ContractError::identity(
            "qualification.predecessor",
            "unexpected_run_reference",
        )),
        (Some(_), None) => Err(ContractError::identity(
            "qualification.predecessor",
            "missing_run_reference",
        )),
        (Some(_), Some(reference)) => reference.validate(),
    }
}

fn validate_campaign(campaign: &str) -> Result<(), ContractError> {
    let valid = !campaign.is_empty()
        && campaign.len() <= 64
        && campaign
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && campaign
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(ContractError::identity(
            "qualification.campaign",
            "invalid_campaign",
        ))
    }
}

fn validate_repository(repository: &str) -> Result<(), ContractError> {
    let parts: Vec<&str> = repository.split('/').collect();
    let valid = parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        });
    if valid {
        Ok(())
    } else {
        Err(ContractError::identity(
            "qualification.repository",
            "invalid_repository",
        ))
    }
}

fn validate_source_binding(context: &QualificationDispatch) -> Result<(), ContractError> {
    if !is_lower_hex_sha(&context.source_sha) || !is_lower_hex_sha(&context.workflow_sha) {
        return Err(ContractError::identity("qualification.sha", "invalid_sha"));
    }
    if context.source_sha != context.workflow_sha || !context.ref_protected {
        return Err(ContractError::identity(
            "qualification.provenance",
            "unprotected_or_unbound_workflow",
        ));
    }
    Ok(())
}

fn validate_ref_binding(context: &QualificationDispatch) -> Result<(), ContractError> {
    if !valid_branch_name(&context.default_branch)
        || context.git_ref != format!("refs/heads/{}", context.default_branch)
    {
        return Err(ContractError::identity(
            "qualification.default_branch",
            "default_branch_mismatch",
        ));
    }
    let Some(branch) = context.git_ref.strip_prefix("refs/heads/") else {
        return Err(ContractError::identity(
            "qualification.ref",
            "not_branch_ref",
        ));
    };
    if branch.is_empty()
        || context.workflow_ref
            != format!(
                "{}/{CI_WORKFLOW_PATH}@{}",
                context.repository, context.git_ref
            )
    {
        return Err(ContractError::identity(
            "qualification.workflow_ref",
            "workflow_ref_mismatch",
        ));
    }
    Ok(())
}

fn validate_run_identity(context: &QualificationDispatch) -> Result<(), ContractError> {
    if context.run_id == 0 || context.run_attempt == 0 {
        Err(ContractError::identity(
            "qualification.run",
            "invalid_run_identity",
        ))
    } else {
        Ok(())
    }
}

/// Conservative branch-name check shared with the event payload binding.
fn valid_branch_name(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with('/')
        && !branch.ends_with('/')
        && !branch.starts_with('.')
        && !branch.ends_with('.')
        && !branch.contains("..")
        && !branch.contains("@{")
        && branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

/// True for a lowercase hexadecimal Git object ID (SHA-1 or SHA-256).
fn is_lower_hex_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "qualification_dispatch_tests.rs"]
mod tests;
