//! Planned-to-observed platform identity binding for task reports.

use serde::{Deserialize, Serialize};

use crate::cachekey::{PlatformInputs, observed_platform_id, platform_id};
use crate::canonical::validate_digest;
use crate::config::RunsOn;
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
        /// Runner environment recorded by the generated observation step.
        runner_environment: PlatformRunnerEnvironment,
        /// Canonical identity recomputed from observed facts.
        observed_platform_id: String,
        /// Exact facts used to derive the observed identity.
        inputs: PlatformInputs,
    },
    /// Runtime facts were unavailable or were not authoritative.
    Unavailable {
        /// Planned identity from the immutable plan.
        planned_platform_id: String,
        /// Runner environment recorded by the generated observation step.
        runner_environment: PlatformRunnerEnvironment,
        /// Closed reason for withholding observed cache identity.
        reason: PlatformUnavailableReason,
    },
}

/// GitHub's runner-provided environment class, carried as typed evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlatformRunnerEnvironment {
    /// A runner provisioned by GitHub Actions.
    GithubHosted,
    /// A repository or organization supplied runner, including Scale Sets.
    SelfHosted,
    /// The observation operation did not obtain a recognized value.
    Unknown,
}

/// Runner class fixed by the generated workflow before a job starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlannedRunnerEnvironment {
    /// A GitHub-hosted runner selected from the exact label catalog.
    GithubHosted,
    /// A custom or Scale Set runner selected by the generated workflow.
    SelfHosted,
}

/// Immutable runner, target, and platform identity selected by generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedPlatform {
    /// Exact hosted label or Scale Set selector emitted in `runs-on`.
    pub runs_on: String,
    /// Runner class fixed by that selector.
    pub runner_environment: PlannedRunnerEnvironment,
    /// Compile or execution target bound by the plan.
    pub target: String,
    /// Canonical digest with image facts explicitly marked unobserved.
    pub platform_id: String,
}

impl PlannedPlatform {
    /// Construct the canonical planned platform record.
    /// # Errors
    pub fn new(runs_on: &str, target: &str) -> Result<Self, ContractError> {
        let runner_environment = runner_environment_for_selector(runs_on)?;
        let inputs = planned_inputs(runs_on, target)?;
        Ok(Self {
            runs_on: runs_on.to_owned(),
            runner_environment,
            target: target.to_owned(),
            platform_id: platform_id(&inputs)?,
        })
    }

    /// Rebuild the canonical plan preimage with its explicit unknown image values.
    /// # Errors
    pub fn inputs(&self) -> Result<PlatformInputs, ContractError> {
        self.validate()?;
        planned_inputs(&self.runs_on, &self.target)
    }

    /// Recompute the identity and selector class from the immutable fields.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if runner_environment_for_selector(&self.runs_on)? != self.runner_environment {
            return Err(ContractError::identity(
                "planned_platform.runner_environment",
                "selector_mismatch",
            ));
        }
        validate_digest(&self.platform_id)?;
        if platform_id(&planned_inputs(&self.runs_on, &self.target)?)? != self.platform_id {
            return Err(ContractError::identity(
                "planned_platform.platform_id",
                "identity_mismatch",
            ));
        }
        Ok(())
    }
}

impl PlatformRunnerEnvironment {
    /// Parse the closed values emitted by `${{ runner.environment }}`.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "github-hosted" => Self::GithubHosted,
            "self-hosted" => Self::SelfHosted,
            _ => Self::Unknown,
        }
    }
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
        runner_environment: PlatformRunnerEnvironment,
        inputs: PlatformInputs,
    ) -> Result<Self, ContractError> {
        validate_digest(planned_platform_id)?;
        if runner_environment != PlatformRunnerEnvironment::GithubHosted {
            return Err(ContractError::identity(
                "platform_binding.runner_environment",
                "observed_requires_github_hosted",
            ));
        }
        let observed_platform_id = observed_platform_id(&inputs)?;
        Ok(Self::Observed {
            planned_platform_id: planned_platform_id.to_owned(),
            runner_environment,
            observed_platform_id,
            inputs,
        })
    }

    /// Build an explicit unavailable binding for the plan identity.
    /// # Errors
    pub fn unavailable(
        planned_platform_id: &str,
        runner_environment: PlatformRunnerEnvironment,
        reason: PlatformUnavailableReason,
    ) -> Result<Self, ContractError> {
        validate_digest(planned_platform_id)?;
        validate_unavailable_reason(runner_environment, reason)?;
        Ok(Self::Unavailable {
            planned_platform_id: planned_platform_id.to_owned(),
            runner_environment,
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
            runner_environment,
            inputs,
            ..
        } = self
        {
            if *runner_environment != PlatformRunnerEnvironment::GithubHosted {
                return Err(ContractError::identity(
                    "platform_binding.runner_environment",
                    "observed_requires_github_hosted",
                ));
            }
            validate_digest(supplied)?;
            if observed_platform_id(inputs)? != *supplied {
                return Err(ContractError::identity(
                    "platform_binding.observed_platform_id",
                    "identity_mismatch",
                ));
            }
        } else if let Self::Unavailable {
            runner_environment,
            reason,
            ..
        } = self
        {
            validate_unavailable_reason(*runner_environment, *reason)?;
        }
        Ok(())
    }

    /// Bind the observation to the exact plan label, target, and identity.
    /// # Errors
    pub fn validate_for_plan(&self, planned: &PlannedPlatform) -> Result<(), ContractError> {
        planned.validate()?;
        self.validate()?;
        if self.planned_platform_id() != planned.platform_id {
            return Err(ContractError::identity(
                "platform_binding.planned_platform_id",
                "plan_mismatch",
            ));
        }
        let recorded_environment = match self {
            Self::Observed {
                runner_environment, ..
            }
            | Self::Unavailable {
                runner_environment, ..
            } => *runner_environment,
        };
        if !environment_matches_plan(self, recorded_environment, planned.runner_environment) {
            return Err(ContractError::identity(
                "platform_binding.runner_environment",
                "plan_mismatch",
            ));
        }
        if let Self::Observed { inputs, .. } = self {
            if inputs.runs_on != planned.runs_on || inputs.target != planned.target {
                return Err(ContractError::identity(
                    "platform_binding.inputs",
                    "plan_mismatch",
                ));
            }
            let mut planned = inputs.clone();
            let unknown = RunnerImageEvidence::unobserved();
            planned.image_os = unknown.image_os;
            planned.image_version = unknown.image_version;
            if platform_id(&planned)? != self.planned_platform_id() {
                return Err(ContractError::identity(
                    "platform_binding.inputs",
                    "planned_identity_mismatch",
                ));
            }
        }
        Ok(())
    }
}

/// Compare runner evidence with the class fixed in the immutable plan.
fn environment_matches_plan(
    binding: &PlatformBinding,
    recorded: PlatformRunnerEnvironment,
    expected: PlannedRunnerEnvironment,
) -> bool {
    match (recorded, expected) {
        (PlatformRunnerEnvironment::GithubHosted, PlannedRunnerEnvironment::GithubHosted)
        | (PlatformRunnerEnvironment::SelfHosted, PlannedRunnerEnvironment::SelfHosted) => true,
        (PlatformRunnerEnvironment::Unknown, _) => matches!(
            binding,
            PlatformBinding::Unavailable {
                reason: PlatformUnavailableReason::InvalidRuntimeFacts
                    | PlatformUnavailableReason::ObservationNotRecorded,
                ..
            }
        ),
        _ => false,
    }
}

/// Construct plan-time facts without inventing runner image metadata.
fn planned_inputs(runs_on: &str, target: &str) -> Result<PlatformInputs, ContractError> {
    let (arch, os) = match target {
        "x86_64-unknown-linux-gnu" => ("x86_64", "linux"),
        "aarch64-apple-darwin" => ("aarch64", "macos"),
        "x86_64-apple-darwin" => ("x86_64", "macos"),
        _ => {
            return Err(ContractError::identity(
                "target",
                format!("unsupported_target:{target}"),
            ));
        }
    };
    match RunsOn::parse(runs_on)? {
        RunsOn::Hosted(_) => {}
        RunsOn::ScaleSet(_) if target == "x86_64-unknown-linux-gnu" => {}
        RunsOn::ScaleSet(_) => {
            return Err(ContractError::identity(
                "planned_platform.target",
                "scale_set_target_mismatch",
            ));
        }
    }
    let unobserved = RunnerImageEvidence::unobserved();
    Ok(PlatformInputs {
        os: os.to_owned(),
        arch: arch.to_owned(),
        runs_on: runs_on.to_owned(),
        image_os: unobserved.image_os,
        image_version: unobserved.image_version,
        target: target.to_owned(),
    })
}

/// True for labels GitHub provisions: the workflow catalog plus the
/// release-target runner map (macOS hosted labels).
fn is_github_hosted_label(label: &str) -> bool {
    crate::config::RUNNER_LABEL_CATALOG.contains(&label)
        || crate::targets::ReleaseTarget::for_runner_label(label).is_some()
}

fn runner_environment_for_selector(
    runs_on: &str,
) -> Result<PlannedRunnerEnvironment, ContractError> {
    match RunsOn::parse(runs_on)? {
        RunsOn::Hosted(label) if is_github_hosted_label(&label) => {
            Ok(PlannedRunnerEnvironment::GithubHosted)
        }
        // Custom ephemeral labels (Mise `EphemeralSelfHosted` checks) are
        // repository-provisioned runners: self-hosted class, so observed
        // hosted identities stay inadmissible for them via `CustomRunner`.
        RunsOn::Hosted(_) | RunsOn::ScaleSet(_) => Ok(PlannedRunnerEnvironment::SelfHosted),
    }
}

/// Keep unavailable reasons coherent with the runner evidence that produced them.
fn validate_unavailable_reason(
    runner_environment: PlatformRunnerEnvironment,
    reason: PlatformUnavailableReason,
) -> Result<(), ContractError> {
    let valid = match reason {
        PlatformUnavailableReason::CustomRunner => {
            runner_environment == PlatformRunnerEnvironment::SelfHosted
        }
        PlatformUnavailableReason::MissingImageMetadata => {
            runner_environment == PlatformRunnerEnvironment::GithubHosted
        }
        PlatformUnavailableReason::InvalidRuntimeFacts
        | PlatformUnavailableReason::RuntimeFactsMismatch
        | PlatformUnavailableReason::ObservationNotRecorded => true,
    };
    if valid {
        Ok(())
    } else {
        Err(ContractError::identity(
            "platform_binding.reason",
            "reason_environment_mismatch",
        ))
    }
}
