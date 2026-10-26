// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_actions_contract::ContractError;

use crate::evidence::{
    Evidence, EvidenceFile, MiseWrapperInput, NextestConfigInput, collect_evidence,
};
use crate::profile_select::{
    check_driver_ambiguity, recommend, select_driver, select_nextest_profile, select_runner,
};

pub const TRANSIENT_EVIDENCE_CODE: &str = "transient_evidence_requires_declaration";

pub const PROFILE_CONFLICT_CODE: &str = "profile_conflict";

pub const AMBIGUOUS_RUNNER_CODE: &str = "ambiguous_test_runner";

pub const AMBIGUOUS_DRIVER_CODE: &str = "ambiguous_compile_driver";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileDriver {
    Cargo,
    Mbx,
}

impl CompileDriver {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ContractError> {
        match value {
            "cargo" => Ok(Self::Cargo),
            "mbx" => Ok(Self::Mbx),
            _ => Err(ContractError::identity(
                "compile_driver",
                format!("unknown_driver:{value}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestRunner {
    CargoTest,
    CargoNextest,
}

impl TestRunner {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CargoTest => "cargo_test",
            Self::CargoNextest => "cargo_nextest",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ContractError> {
        match value {
            "cargo_test" => Ok(Self::CargoTest),
            "cargo_nextest" => Ok(Self::CargoNextest),
            _ => Err(ContractError::identity(
                "test_runner",
                format!("unknown_runner:{value}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextestProfile {
    Ci,
    Default,
}

impl NextestProfile {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ci => "ci",
            Self::Default => "default",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ContractError> {
        match value {
            "ci" => Ok(Self::Ci),
            "default" => Ok(Self::Default),
            _ => Err(ContractError::identity(
                "nextest_profile",
                format!("unknown_profile:{value}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    Declared,
    Detected,
}

impl ProfileSource {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Declared => "declared",
            Self::Detected => "detected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustExecutionProfile {
    pub compile_driver: CompileDriver,
    pub test_runner: TestRunner,
    pub evidence: Vec<Evidence>,
    pub driver_source: ProfileSource,
    pub runner_source: ProfileSource,
    pub nextest_profile: NextestProfile,
    pub nextest_config: Option<String>,
    pub run_ignored: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileFinding {
    pub code: String,
    pub message: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommendation {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileOutcome {
    pub profile: RustExecutionProfile,
    pub recommendations: Vec<Recommendation>,
    pub findings: Vec<ProfileFinding>,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileInputs<'a> {
    pub tool_config: Option<EvidenceFile<'a>>,
    pub cargo_configs: Vec<EvidenceFile<'a>>,
    pub executables: Vec<EvidenceFile<'a>>,
    pub handwritten_workflows: Vec<EvidenceFile<'a>>,
    pub declared_driver: Option<CompileDriver>,
    pub declared_runner: Option<TestRunner>,
    pub run_ignored: Option<String>,
    pub mise_wrappers: Vec<MiseWrapperInput>,
    pub nextest_configs: Vec<NextestConfigInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    AmbiguousTestRunner {
        evidence: Vec<Evidence>,
    },
    AmbiguousDriver {
        evidence: Vec<Evidence>,
    },
    ProfileConflict {
        declared: String,
        evidence: Vec<Evidence>,
    },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AmbiguousTestRunner { evidence } => {
                write!(f, "{AMBIGUOUS_RUNNER_CODE}: {} sightings", evidence.len())
            }
            Self::AmbiguousDriver { evidence } => {
                write!(f, "{AMBIGUOUS_DRIVER_CODE}: {} sightings", evidence.len())
            }
            Self::ProfileConflict { declared, evidence } => {
                write!(
                    f,
                    "{PROFILE_CONFLICT_CODE}: declared {declared} contradicts "
                )?;
                for sighting in evidence {
                    write!(
                        f,
                        "{}:{} {} ",
                        sighting.path, sighting.line, sighting.command_or_setting
                    )?;
                }
                write!(f, "update the declaration or the durable signal")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

pub fn detect_profile(inputs: &ProfileInputs<'_>) -> Result<ProfileOutcome, ProfileError> {
    let seen = collect_evidence(
        inputs.tool_config.as_ref(),
        &inputs.cargo_configs,
        &inputs.executables,
        &inputs.handwritten_workflows,
        &inputs.mise_wrappers,
        &inputs.nextest_configs,
    );
    if !seen.nextest_durable.is_empty() && !seen.cargo_durable.is_empty() {
        let mut conflicting = seen.nextest_durable.clone();
        conflicting.extend(seen.cargo_durable.clone());
        conflicting.sort();
        return Err(ProfileError::AmbiguousTestRunner {
            evidence: conflicting,
        });
    }
    check_driver_ambiguity(&seen, &inputs.mise_wrappers)?;
    let mut findings = Vec::new();
    let (compile_driver, driver_source) = select_driver(&seen, inputs, &mut findings)?;
    let (test_runner, runner_source) = select_runner(&seen, inputs, &mut findings)?;
    let (nextest_profile, nextest_config) = select_nextest_profile(inputs);
    let recommendations = recommend(inputs, &seen);
    let evidence = seen.all_sorted();
    Ok(ProfileOutcome {
        profile: RustExecutionProfile {
            compile_driver,
            test_runner,
            evidence,
            driver_source,
            runner_source,
            nextest_profile,
            nextest_config,
            run_ignored: inputs.run_ignored.clone(),
        },
        recommendations,
        findings,
    })
}
