//! Per-workspace execution-profile selection with provenance.
//!
//! Durable evidence selects profiles silently. A non-default selection resting
//! solely on transient evidence yields a
//! [`TRANSIENT_EVIDENCE_CODE`] finding: `plan` reports it, `generate` fails
//! closed before touching `.github`. Declared `[stacks.rust]` keys are sticky
//! across regenerations; durable evidence contradicting them fails closed with
//! [`PROFILE_CONFLICT_CODE`], never a silent flip either direction.

use std::fmt;

use crate::evidence::{
    Evidence, EvidenceFile, NEXTEST_RECOMMENDATION, PERSIST_EVIDENCE, Seen, collect_evidence,
};

/// Finding when transient-only evidence selects a non-default profile.
pub const TRANSIENT_EVIDENCE_CODE: &str = "transient_evidence_requires_declaration";

/// Error when durable evidence contradicts a declared key.
pub const PROFILE_CONFLICT_CODE: &str = "profile_conflict";

/// Error when both test runners are explicitly used.
pub const AMBIGUOUS_RUNNER_CODE: &str = "ambiguous_test_runner";

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
}

/// Profile detection failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// Both test runners are explicitly used.
    AmbiguousTestRunner {
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
/// durably used, and [`ProfileError::ProfileConflict`] when durable evidence
/// contradicts a declared key.
pub fn detect_profile(inputs: &ProfileInputs<'_>) -> Result<ProfileOutcome, ProfileError> {
    let seen = collect_evidence(
        inputs.tool_config.as_ref(),
        &inputs.cargo_configs,
        &inputs.executables,
        &inputs.handwritten_workflows,
    );
    if !seen.nextest_durable.is_empty() && !seen.cargo_durable.is_empty() {
        let mut conflicting = seen.nextest_durable.clone();
        conflicting.extend(seen.cargo_durable.clone());
        conflicting.sort();
        return Err(ProfileError::AmbiguousTestRunner {
            evidence: conflicting,
        });
    }
    let mut findings = Vec::new();
    let (compile_driver, driver_source) = select_driver(&seen, inputs, &mut findings)?;
    let (test_runner, runner_source) = select_runner(&seen, inputs, &mut findings)?;
    let recommendations = recommend(inputs, &seen);
    let evidence = seen.all_sorted();
    Ok(ProfileOutcome {
        profile: RustExecutionProfile {
            compile_driver,
            test_runner,
            evidence,
            driver_source,
            runner_source,
        },
        recommendations,
        findings,
    })
}

/// Select the compile driver: declared wins, else durable, else transient+finding.
fn select_driver(
    seen: &Seen,
    inputs: &ProfileInputs<'_>,
    findings: &mut Vec<ProfileFinding>,
) -> Result<(CompileDriver, ProfileSource), ProfileError> {
    if let Some(declared) = inputs.declared_driver {
        if declared == CompileDriver::Cargo && !seen.mbx_durable.is_empty() {
            return Err(ProfileError::ProfileConflict {
                declared: "compile_driver = \"cargo\"".to_owned(),
                evidence: sorted(seen.mbx_durable.clone()),
            });
        }
        return Ok((declared, ProfileSource::Declared));
    }
    if !seen.mbx_durable.is_empty() {
        return Ok((CompileDriver::Mbx, ProfileSource::Detected));
    }
    if !seen.mbx_transient.is_empty() {
        findings.push(transient_finding(
            "compile driver",
            "compile_driver",
            CompileDriver::Mbx.as_str(),
            sorted(seen.mbx_transient.clone()),
        ));
        return Ok((CompileDriver::Mbx, ProfileSource::Detected));
    }
    Ok((CompileDriver::Cargo, ProfileSource::Detected))
}

/// Select the test runner: declared wins, else durable, else transient+finding.
fn select_runner(
    seen: &Seen,
    inputs: &ProfileInputs<'_>,
    findings: &mut Vec<ProfileFinding>,
) -> Result<(TestRunner, ProfileSource), ProfileError> {
    if let Some(declared) = inputs.declared_runner {
        check_runner_conflict(seen, declared)?;
        return Ok((declared, ProfileSource::Declared));
    }
    if !seen.nextest_durable.is_empty() {
        return Ok((TestRunner::CargoNextest, ProfileSource::Detected));
    }
    if !seen.cargo_durable.is_empty() {
        return Ok((TestRunner::CargoTest, ProfileSource::Detected));
    }
    if !seen.nextest_transient.is_empty() {
        findings.push(transient_finding(
            "test runner",
            "test_runner",
            TestRunner::CargoNextest.as_str(),
            sorted(seen.nextest_transient.clone()),
        ));
        return Ok((TestRunner::CargoNextest, ProfileSource::Detected));
    }
    Ok((TestRunner::CargoTest, ProfileSource::Detected))
}

/// Fail when durable runner evidence contradicts the declared runner.
fn check_runner_conflict(seen: &Seen, declared: TestRunner) -> Result<(), ProfileError> {
    let (key, contradicting) = match declared {
        TestRunner::CargoTest => ("test_runner = \"cargo_test\"", seen.nextest_durable.clone()),
        TestRunner::CargoNextest => (
            "test_runner = \"cargo_nextest\"",
            seen.cargo_durable.clone(),
        ),
    };
    if contradicting.is_empty() {
        return Ok(());
    }
    Err(ProfileError::ProfileConflict {
        declared: key.to_owned(),
        evidence: sorted(contradicting),
    })
}

/// Sort one evidence bucket.
fn sorted(mut evidence: Vec<Evidence>) -> Vec<Evidence> {
    evidence.sort();
    evidence
}

/// Build the blocking finding for a transient-only non-default selection.
fn transient_finding(
    axis: &str,
    key: &str,
    value: &str,
    mut evidence: Vec<Evidence>,
) -> ProfileFinding {
    evidence.sort();
    ProfileFinding {
        code: TRANSIENT_EVIDENCE_CODE.to_owned(),
        message: format!(
            "{axis} rests on transient evidence only ({} sightings); \
(a) declare [stacks.rust] {key} = \"{value}\", \
or (b) move the invocation to a durable executable task outside .github",
            evidence.len()
        ),
        evidence,
    }
}

fn recommend(inputs: &ProfileInputs<'_>, seen: &Seen) -> Vec<Recommendation> {
    let mut out = Vec::new();
    if inputs.declared_runner.is_none() && !seen.has_runner() {
        out.push(Recommendation {
            code: NEXTEST_RECOMMENDATION.to_owned(),
            message: "no explicit test-runner usage; defaulting to `cargo test`".to_owned(),
        });
    }
    let declared = inputs.declared_driver.is_some() && inputs.declared_runner.is_some();
    if !seen.has_durable() && !declared {
        let message = if seen.is_empty() {
            "no MBX or Nextest usage detected; to adopt either, add a durable signal"
        } else {
            "profile rests on hand-written workflows that generation replaces; persist a durable signal"
        };
        out.push(Recommendation {
            code: PERSIST_EVIDENCE.to_owned(),
            message: message.to_owned(),
        });
    }
    out
}
