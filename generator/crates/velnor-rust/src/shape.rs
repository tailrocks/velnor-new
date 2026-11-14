// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Shape, ShapeDecision, Timings};

use crate::config::CONFIG_PATH;
use crate::timings::TIMINGS_PATH;

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
