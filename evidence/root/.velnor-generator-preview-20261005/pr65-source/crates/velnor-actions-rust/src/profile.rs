//! Per-workspace execution-profile selection with provenance.
//!
//! Durable evidence selects profiles silently. A non-default selection resting
//! solely on transient evidence yields a
//! [`TRANSIENT_EVIDENCE_CODE`] finding: `plan` reports it, `generate` fails
//! closed before touching `.github`. Declared `[stacks.rust]` keys are sticky
//! across regenerations; durable evidence contradicting them fails closed with
//! [`PROFILE_CONFLICT_CODE`], never a silent flip either direction.

use std::fmt;

use velnor_actions_contract::ContractError;

use crate::evidence::{
    Evidence, EvidenceFile, MiseWrapperInput, NextestConfigInput, collect_evidence,
};
use crate::profile_select::{
    check_driver_ambiguity, recommend, select_driver, select_nextest_profile, select_runner,
};

/// Finding when transient-only evidence selects a non-default profile.
pub const TRANSIENT_EVIDENCE_CODE: &str = "transient_evidence_requires_declaration";

/// Error when durable evidence contradicts a declared key.
pub const PROFILE_CONFLICT_CODE: &str = "profile_conflict";

/// Error when both test runners are explicitly used.
pub const AMBIGUOUS_RUNNER_CODE: &str = "ambiguous_test_runner";

/// Error when compile-driver signals contradict each other.
pub const AMBIGUOUS_DRIVER_CODE: &str = "ambiguous_compile_driver";

/// Selected compile driver for one workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileDriver {
    /// Plain Cargo compilation.
    Cargo,
    /// MBX compilation.
    Mbx,
}

impl CompileDriver {
    /// Stable profile name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }

    /// Parse a driver token; unknown tokens fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for any token outside `cargo`/`mbx`.
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

/// Selected test runner for one workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestRunner {
    /// Plain `cargo test`.
    CargoTest,
    /// Nextest execution.
    CargoNextest,
}

impl TestRunner {
    /// Stable profile name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CargoTest => "cargo_test",
            Self::CargoNextest => "cargo_nextest",
        }
    }

    /// Parse a runner token; unknown tokens fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for any token outside
    /// `cargo_test`/`cargo_nextest`.
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

/// Selected Nextest profile for one workspace.
///
/// Meaningful when the test runner is Nextest: `ci` when the nearest
/// `.config/nextest.toml` declares `[profile.ci]`, else Nextest's
/// documented `default` profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextestProfile {
    /// `[profile.ci]` declared in the nearest Nextest config.
    Ci,
    /// Documented default (no `[profile.ci]` in the nearest config).
    Default,
}

impl NextestProfile {
    /// Stable profile name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ci => "ci",
            Self::Default => "default",
        }
    }

    /// Parse a resolved profile token; unknown tokens fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for any token outside `ci`/`default`.
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

/// Where one profile axis came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    /// Explicit `[stacks.rust]` declaration; sticky across regenerations.
    Declared,
    /// Detected from evidence or the documented default.
    Detected,
}

impl ProfileSource {
    /// Stable provenance name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Declared => "declared",
            Self::Detected => "detected",
        }
    }
}

/// Per-workspace execution profile with provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustExecutionProfile {
    /// Selected compile driver.
    pub compile_driver: CompileDriver,
    /// Selected test runner.
    pub test_runner: TestRunner,
    /// Evidence backing the selection, sorted by `(path, line)`.
    pub evidence: Vec<Evidence>,
    /// Provenance of the compile driver.
    pub driver_source: ProfileSource,
    /// Provenance of the test runner.
    pub runner_source: ProfileSource,
    /// Selected Nextest profile (meaningful for Nextest runners).
    pub nextest_profile: NextestProfile,
    /// Nearest consumed `.config/nextest.toml`, when any exists.
    pub nextest_config: Option<String>,
    /// Ignored test execution mode ("all", "only", "ignored-only", "default").
    pub run_ignored: Option<String>,
}

/// Blocking finding: transient-only evidence needs an explicit declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileFinding {
    /// Stable finding code ([`TRANSIENT_EVIDENCE_CODE`]).
    pub code: String,
    /// Human-readable guidance naming both remedies.
    pub message: String,
    /// Transient evidence the blocked selection rests on, sorted.
    pub evidence: Vec<Evidence>,
}

/// Advisory recommendation emitted with a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommendation {
    /// Stable recommendation code.
    pub code: String,
    /// Human-readable guidance.
    pub message: String,
}

/// Detected profile plus recommendations and blocking findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileOutcome {
    /// Selected profile.
    pub profile: RustExecutionProfile,
    /// Advisory recommendations.
    pub recommendations: Vec<Recommendation>,
    /// Blocking findings; `generate` MUST fail closed when non-empty.
    pub findings: Vec<ProfileFinding>,
}

/// Inputs for profile detection (all bytes come from the orchestrator).
#[derive(Debug, Clone, Default)]
pub struct ProfileInputs<'a> {
    /// Tool-config content holding the Rust `mr_boxington` setting, if any.
    pub tool_config: Option<EvidenceFile<'a>>,
    /// Cargo config contents inspected for `rustc-wrapper` naming MBX.
    pub cargo_configs: Vec<EvidenceFile<'a>>,
    /// Executable task and script contents.
    pub executables: Vec<EvidenceFile<'a>>,
    /// Hand-written workflow contents (never generated output).
    pub handwritten_workflows: Vec<EvidenceFile<'a>>,
    /// Declared `[stacks.rust] compile_driver`, if any.
    pub declared_driver: Option<CompileDriver>,
    /// Declared `[stacks.rust] test_runner`, if any.
    pub declared_runner: Option<TestRunner>,
    /// Declared `[stacks.rust] run_ignored`, if any.
    pub run_ignored: Option<String>,
    /// Structurally resolved Mise Cargo wrappers (all inspected files).
    pub mise_wrappers: Vec<MiseWrapperInput>,
    /// Structurally resolved Nextest configs, nearest first.
    pub nextest_configs: Vec<NextestConfigInput>,
}

/// Profile detection failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// Both test runners are explicitly used.
    AmbiguousTestRunner {
        /// Conflicting evidence, sorted.
        evidence: Vec<Evidence>,
    },
    /// Compile-driver signals contradict each other.
    AmbiguousDriver {
        /// Conflicting evidence, sorted.
        evidence: Vec<Evidence>,
    },
    /// Durable evidence contradicts a declared key.
    ProfileConflict {
        /// Declared key and value (`compile_driver = "cargo"`).
        declared: String,
        /// Contradicting durable evidence, sorted.
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

/// Detect the per-workspace profile from durability-tagged evidence.
///
/// # Errors
///
/// Returns [`ProfileError::AmbiguousTestRunner`] when both test runners are
/// durably used, [`ProfileError::AmbiguousDriver`] when driver signals
/// contradict, and [`ProfileError::ProfileConflict`] when durable
/// evidence contradicts a declared key.
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
