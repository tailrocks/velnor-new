//! Run-derived identity for verification-only repeats.

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::workflow::QUALIFICATION_WORKFLOW_PATH;
use crate::workflow::qualification_phase::QualificationPhase;

use super::{QualificationDispatch, QualificationRunRef};

/// Dispatch flow recorded in the immutable qualification context.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDispatchMode {
    /// Caller-selected qualification flow with explicit campaign and phase.
    #[default]
    Controlled,
    /// Run-derived cold and warm sequence through the qualification wrapper.
    VerificationOnly,
}

impl QualificationDispatchMode {
    /// Whether this value selects the caller-controlled flow.
    #[must_use]
    pub const fn is_controlled(&self) -> bool {
        matches!(self, Self::Controlled)
    }
}

pub(super) fn validate(context: &QualificationDispatch) -> Result<(), ContractError> {
    if context.mode == QualificationDispatchMode::Controlled {
        return Ok(());
    }
    if super::workflow_path(&context.workflow_ref, &context.repository)
        != Some(QUALIFICATION_WORKFLOW_PATH)
    {
        return Err(ContractError::identity(
            "qualification.mode",
            "verification_only_requires_wrapper",
        ));
    }
    let (phase, predecessor) = if context.run_attempt == 1 {
        (QualificationPhase::Cold, None)
    } else {
        (
            QualificationPhase::Warm,
            Some(QualificationRunRef {
                run_id: context.run_id,
                run_attempt: 1,
            }),
        )
    };
    if context.phase != phase || context.predecessor != predecessor {
        return Err(ContractError::identity(
            "qualification.mode",
            "repeat_phase_mismatch",
        ));
    }
    let campaign = verification_repeat_campaign(
        &context.repository,
        &context.default_branch,
        &context.git_ref,
        &context.workflow_ref,
        &context.workflow_sha,
        &context.source_sha,
        context.run_id,
    );
    if context.campaign != campaign {
        return Err(ContractError::identity(
            "qualification.mode",
            "repeat_campaign_mismatch",
        ));
    }
    Ok(())
}

/// Build the stable campaign ID from the immutable source and original run.
#[must_use]
pub fn verification_repeat_campaign(
    repository: &str,
    default_branch: &str,
    git_ref: &str,
    workflow_ref: &str,
    workflow_sha: &str,
    source_sha: &str,
    run_id: u64,
) -> String {
    let material = format!(
        "velnor-verification-repeat-campaign-v1\0{repository}\0{default_branch}\0{git_ref}\0{workflow_ref}\0{workflow_sha}\0{source_sha}\0{run_id}"
    );
    let digest = blake3::hash(material.as_bytes()).to_hex().to_string();
    format!("vr1-{}", &digest[..60])
}
