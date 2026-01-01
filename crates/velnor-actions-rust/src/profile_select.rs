//! Profile-axis selection over durability-tagged evidence.
//!
//! The driver and runner axes stay independent: on each axis a declared
//! `[stacks.rust]` key wins, else durable evidence, else transient
//! evidence plus a blocking finding, else the documented default.
//! Contradictions fail closed; the Nextest profile follows the nearest
//! config (`ci` when declared, else the documented default).

use crate::evidence::{
    Evidence, MiseWrapperInput, NEXTEST_RECOMMENDATION, PERSIST_EVIDENCE, SHADOWED_NEXTEST_CONFIG,
    Seen, wrapper_sightings,
};
use crate::profile::{
    CompileDriver, NextestProfile, ProfileError, ProfileFinding, ProfileInputs, ProfileSource,
    Recommendation, TRANSIENT_EVIDENCE_CODE, TestRunner,
};

/// Select the compile driver: declared wins, else durable, else transient+finding.
pub(crate) fn select_driver(
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
        if declared == CompileDriver::Mbx && !seen.wrapper_other.is_empty() {
            return Err(ProfileError::ProfileConflict {
                declared: "compile_driver = \"mbx\"".to_owned(),
                evidence: sorted(seen.wrapper_other.clone()),
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
pub(crate) fn select_runner(
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

/// Advisory recommendations for the detected selection.
pub(crate) fn recommend(inputs: &ProfileInputs<'_>, seen: &Seen) -> Vec<Recommendation> {
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
    if let Some((nearest, rest)) = inputs.nextest_configs.split_first()
        && !rest.is_empty()
    {
        let far: Vec<&str> = rest.iter().map(|input| input.path.as_str()).collect();
        out.push(Recommendation {
            code: SHADOWED_NEXTEST_CONFIG.to_owned(),
            message: format!(
                "{} shadowed by {}; profile selected from {}",
                far.join(", "),
                nearest.path,
                nearest.path
            ),
        });
    }
    out
}

/// Fail when wrapper commands contradict each other or MBX sightings.
///
/// Distinct `wrappers.cargo.command` values across the inspected files
/// cannot all hold (one workspace, one compile route); likewise a
/// durable MBX signal contradicts an explicit non-MBX wrapper.
pub(crate) fn check_driver_ambiguity(
    seen: &Seen,
    wrappers: &[MiseWrapperInput],
) -> Result<(), ProfileError> {
    let mut commands: Vec<&str> = wrappers
        .iter()
        .map(|input| input.command.as_str())
        .collect();
    commands.sort_unstable();
    commands.dedup();
    if commands.len() > 1 {
        let (mbx, other) = wrapper_sightings(wrappers);
        let mut conflicting = mbx;
        conflicting.extend(other);
        conflicting.sort();
        return Err(ProfileError::AmbiguousDriver {
            evidence: conflicting,
        });
    }
    if !seen.mbx_durable.is_empty() && !seen.wrapper_other.is_empty() {
        let mut conflicting = seen.mbx_durable.clone();
        conflicting.extend(seen.wrapper_other.clone());
        conflicting.sort();
        return Err(ProfileError::AmbiguousDriver {
            evidence: conflicting,
        });
    }
    Ok(())
}

/// Select the Nextest profile from the nearest config.
///
/// Inputs arrive nearest-first: the first config selects `ci` when it
/// declares `[profile.ci]`, else Nextest's documented `default` profile.
pub(crate) fn select_nextest_profile(
    inputs: &ProfileInputs<'_>,
) -> (NextestProfile, Option<String>) {
    let Some(nearest) = inputs.nextest_configs.first() else {
        return (NextestProfile::Default, None);
    };
    let profile = if nearest.ci_line.is_some() {
        NextestProfile::Ci
    } else {
        NextestProfile::Default
    };
    (profile, Some(nearest.path.clone()))
}
