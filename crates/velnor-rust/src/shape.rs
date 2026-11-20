// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoFacts, RustRoot, Shape, ShapeDecision, Timings};

use crate::config::CONFIG_PATH;
use crate::error::Reason;
use crate::timings::TIMINGS_PATH;
use crate::units::{Unit, units};

pub const SECOND_JOB_COST_SECONDS: u64 = 5;
pub const SHARD_THRESHOLD_SECONDS: Option<u64> = None;
const CHECK_ALONE_PULL_REQUEST_LIMIT_SECONDS: u64 = 30;
const CHECK_ALONE_MEASURED_SECONDS: u64 = 1;

const _: () = assert!(CHECK_ALONE_MEASURED_SECONDS <= CHECK_ALONE_PULL_REQUEST_LIMIT_SECONDS);

#[must_use]
pub fn decide_shape(configured: Option<Shape>, timings: Option<&Timings>) -> ShapeDecision {
    let decision = |shape, reason: String| ShapeDecision { shape, reason };
    if let Some(shape) = configured {
        let name = match shape {
            Shape::OneJob => "one-job",
            Shape::LintAndTest => "lint-test",
            Shape::BuildThenGroups => "groups",
        };
        return decision(shape, format!("shape = \"{name}\" in {CONFIG_PATH}"));
    }
    let Some(timings) = timings else {
        return decision(
            Shape::OneJob,
            format!("one job: {TIMINGS_PATH} has no build times for this workspace"),
        );
    };
    if let Some(threshold) = SHARD_THRESHOLD_SECONDS.filter(|t| timings.test_seconds > *t) {
        return decision(
            Shape::BuildThenGroups,
            format!(
                "build once, then test groups: the tests take {} s, longer than the {threshold} s shard threshold (#585)",
                timings.test_seconds
            ),
        );
    }
    let shorter = timings.lint_build_seconds.min(timings.test_build_seconds);
    if shorter > SECOND_JOB_COST_SECONDS {
        decision(
            Shape::LintAndTest,
            format!(
                "lint and test jobs: the shorter build takes {shorter} s, longer than the {SECOND_JOB_COST_SECONDS} s a second job adds (#585)"
            ),
        )
    } else {
        decision(
            Shape::OneJob,
            format!(
                "one job: the shorter build takes {shorter} s, not longer than the {SECOND_JOB_COST_SECONDS} s a second job adds (#585)"
            ),
        )
    }
}

/// The note for `shape = "auto"` when a root has no build times. Auto then picks the
/// one-job shape without measuring anything, which the generator says out loud.
#[must_use]
pub fn untimed_auto_note(facts: &RepoFacts) -> Option<String> {
    if facts.config.shape.is_some() {
        return None;
    }
    let units = units(facts);
    let roots: Vec<String> = units
        .iter()
        .zip(decisions(facts, &units))
        .filter(|(unit, decision)| {
            decision.shape == Shape::OneJob && crate::units::timings(facts, unit).is_none()
        })
        .flat_map(|(unit, _)| &unit.roots)
        .filter(|root| !facts.timings.contains_key(&root.manifest))
        .map(|root| root.manifest.to_string())
        .collect();
    (!roots.is_empty()).then(|| {
        format!(
            "shape = \"auto\" picked the one-job shape for {}: {TIMINGS_PATH} has no build times for them (the file is missing or has no entry for them), so clippy and the tests run in one job. To choose a shape, set shape = \"lint-test\" (or \"groups\") in {CONFIG_PATH}; to let auto measure, once the generated workflow has three successful runs on the default branch, run `velnor-ci timings --repo owner/name` in the checkout (it reads GITHUB_TOKEN) and commit {TIMINGS_PATH}.",
            roots.join(", ")
        )
    })
}

/// The shape decision of every unit, as the renderer applies it.
pub(crate) fn decisions(facts: &RepoFacts, units: &[Unit]) -> Vec<ShapeDecision> {
    units
        .iter()
        .map(|unit| {
            let decision = decide_shape(
                facts.config.shape,
                crate::units::timings(facts, unit).as_ref(),
            );
            if unit.grouped {
                decision
            } else {
                crate::config_test_groups::archive_shape(facts, &unit.roots[0], decision)
            }
        })
        .collect()
}

pub(crate) fn helper_root(facts: &RepoFacts) -> Option<&RustRoot> {
    facts.config.helper.as_ref().and_then(|path| {
        facts
            .rust_roots
            .iter()
            .find(|root| root.manifest.parent().as_ref() == Some(path))
    })
}

/// One `rust` job and no plan job: the single-job shape.
pub(crate) fn single_job(facts: &RepoFacts, units: &[Unit], decisions: &[ShapeDecision]) -> bool {
    units.len() == 1
        && !units[0].grouped
        && decisions[0].shape == Shape::OneJob
        && helper_root(facts).is_none()
        && !crate::config_test_lane::has_parallel(&facts.config)
}

/// Whether the planned workflow has a plan job. Only the jobs that follow a plan
/// job get a sparse checkout list (see `setup_units`), so the one-job shape,
/// which has none, never gets a sparse suggestion (#645).
#[must_use]
pub fn sparse_checkout_planned(facts: &RepoFacts) -> bool {
    let units = units(facts);
    let decisions = decisions(facts, &units);
    !single_job(facts, &units, &decisions)
}

pub(crate) fn parse_shape(configured: Option<&str>) -> Result<Option<Shape>, Reason> {
    match configured {
        None | Some("auto") => Ok(None),
        Some("one-job") => Ok(Some(Shape::OneJob)),
        Some("lint-test") => Ok(Some(Shape::LintAndTest)),
        Some("groups") => Ok(Some(Shape::BuildThenGroups)),
        Some(other) => Err(Reason::InvalidValue {
            key: "shape",
            value: other.to_owned(),
        }),
    }
}
