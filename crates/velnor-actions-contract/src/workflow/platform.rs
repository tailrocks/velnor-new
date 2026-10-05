//! Planned-to-observed platform identity binding for task reports.

use serde::{Deserialize, Serialize};

use crate::cachekey::{PlatformInputs, observed_platform_id, platform_id};
use crate::canonical::validate_digest;
use crate::errors::ContractError;
use crate::freshness::RunnerImageEvidence;

/// Runtime platform evidence attached to every task report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlatformBinding {
    /// Complete observed image facts and their canonical identity.
    Observed {
        /// Planned identity from the immutable plan.
        planned_platform_id: String,
        /// Canonical identity recomputed from observed facts.
        observed_platform_id: String,
        /// Exact facts used to derive the observed identity.
        inputs: PlatformInputs,
    },
    /// Runtime facts were unavailable or were not authoritative.
    Unavailable {
        /// Planned identity from the immutable plan.
        planned_platform_id: String,
        /// Closed reason for withholding observed cache identity.
        reason: PlatformUnavailableReason,
    },
}

/// Why a task has no admitted runtime platform identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformUnavailableReason {
    /// The job ran on a self-hosted/custom runner without immutable image authority.
    CustomRunner,
    /// The hosted runner did not expose both image fields.
    MissingImageMetadata,
    /// One or more runtime facts failed strict validation.
    InvalidRuntimeFacts,
    /// Runner OS or architecture did not match the planned target.
    RuntimeFactsMismatch,
    /// The observation step did not run or did not publish its output.
    ObservationNotRecorded,
}

impl PlatformBinding {
    /// Build an observed binding and derive its ID from canonical facts.
    /// # Errors
    pub fn observed(
        planned_platform_id: &str,
        inputs: PlatformInputs,
    ) -> Result<Self, ContractError> {
        validate_digest(planned_platform_id)?;
        let observed_platform_id = observed_platform_id(&inputs)?;
        Ok(Self::Observed {
            planned_platform_id: planned_platform_id.to_owned(),
            observed_platform_id,
            inputs,
        })
    }

    /// Build an explicit unavailable binding for the plan identity.
    /// # Errors
    pub fn unavailable(
        planned_platform_id: &str,
        reason: PlatformUnavailableReason,
    ) -> Result<Self, ContractError> {
        validate_digest(planned_platform_id)?;
        Ok(Self::Unavailable {
            planned_platform_id: planned_platform_id.to_owned(),
            reason,
        })
    }

    /// Planned identity this observation was requested to bind.
    #[must_use]
    pub fn planned_platform_id(&self) -> &str {
        match self {
            Self::Observed {
                planned_platform_id,
                ..
            }
            | Self::Unavailable {
                planned_platform_id,
                ..
            } => planned_platform_id,
        }
    }

    /// Validate field shapes and recompute any observed digest.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_digest(self.planned_platform_id())?;
        if let Self::Observed {
            observed_platform_id: supplied,
            inputs,
            ..
        } = self
        {
            validate_digest(supplied)?;
            if observed_platform_id(inputs)? != *supplied {
                return Err(ContractError::identity(
                    "platform_binding.observed_platform_id",
                    "identity_mismatch",
                ));
            }
        }
        Ok(())
    }

    /// Bind the observation to the exact plan label, target, and identity.
    /// # Errors
    pub fn validate_for_plan(
        &self,
        planned_platform_id: &str,
        runs_on: &str,
        target: &str,
    ) -> Result<(), ContractError> {
        self.validate()?;
        if self.planned_platform_id() != planned_platform_id {
            return Err(ContractError::identity(
                "platform_binding.planned_platform_id",
                "plan_mismatch",
            ));
        }
        if let Self::Observed { inputs, .. } = self {
            if inputs.runs_on != runs_on || inputs.target != target {
                return Err(ContractError::identity(
                    "platform_binding.inputs",
                    "plan_mismatch",
                ));
            }
            let mut planned = inputs.clone();
            let unknown = RunnerImageEvidence::unobserved();
            planned.image_os = unknown.image_os;
            planned.image_version = unknown.image_version;
            if platform_id(&planned)? != planned_platform_id {
                return Err(ContractError::identity(
                    "platform_binding.inputs",
                    "planned_identity_mismatch",
                ));
            }
        }
        Ok(())
    }
}
