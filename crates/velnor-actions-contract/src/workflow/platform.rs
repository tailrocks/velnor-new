// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::cachekey::{PlatformInputs, observed_platform_id, platform_id};
use crate::canonical::validate_digest;
use crate::config::RunsOn;
use crate::errors::ContractError;
use crate::freshness::RunnerImageEvidence;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlatformBinding {
    Observed {
        planned_platform_id: String,

        runner_environment: PlatformRunnerEnvironment,

        observed_platform_id: String,

        inputs: PlatformInputs,
    },

    Unavailable {
        planned_platform_id: String,

        runner_environment: PlatformRunnerEnvironment,

        reason: PlatformUnavailableReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlatformRunnerEnvironment {
    GithubHosted,

    SelfHosted,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlannedRunnerEnvironment {
    GithubHosted,

    SelfHosted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedPlatform {
    pub runs_on: String,

    pub runner_environment: PlannedRunnerEnvironment,

    pub target: String,

    pub platform_id: String,
}

impl PlannedPlatform {
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

    pub fn inputs(&self) -> Result<PlatformInputs, ContractError> {
        self.validate()?;
        planned_inputs(&self.runs_on, &self.target)
    }

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
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "github-hosted" => Self::GithubHosted,
            "self-hosted" => Self::SelfHosted,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformUnavailableReason {
    CustomRunner,

    MissingImageMetadata,

    InvalidRuntimeFacts,

    RuntimeFactsMismatch,

    ObservationNotRecorded,
}

impl PlatformBinding {
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
        target: target.to_owned(),
    })
}

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

        RunsOn::Hosted(_) | RunsOn::ScaleSet(_) => Ok(PlannedRunnerEnvironment::SelfHosted),
    }
}

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
