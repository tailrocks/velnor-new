// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{FilterRef, TEST_STEP};

use crate::cargo::{DOC_RUN, clippy, feature_flag, nextest_run, without_binaries};
use crate::rules::Value;

/// The default step may select nothing, because the plan narrows it to the
/// changed packages. A feature lane names its tests, so none matching is a
/// broken filter and must not pass.
pub(crate) const NO_TESTS_PASS: &str = "pass";
pub(crate) const NO_TESTS_FAIL: &str = "fail";

pub(crate) fn nextest_features(
    nextest: &str,
    features: &[String],
    filter: &FilterRef,
    ci_profile: bool,
) -> (String, String, Vec<(String, Value)>) {
    let (_, run, env) = nextest_run(
        &format!("--workspace --locked{nextest}{}", feature_flag(features)),
        filter,
        ci_profile,
        NO_TESTS_FAIL,
    );
    (
        format!("{TEST_STEP} (features: {})", features.join(",")),
        run,
        env,
    )
}

pub(crate) fn expr(expr: &str, except: &[String]) -> String {
    let both = format!("($TEST_FILTER) and ({expr})");
    if except.is_empty() {
        both
    } else {
        without_binaries(format!("({both})"), except)
    }
}

pub(crate) fn clippy_features(
    cargo: &str,
    features: &[String],
) -> (String, String, Vec<(String, Value)>) {
    let (name, run, env) = clippy(&format!("{cargo}{}", feature_flag(features)), &[]);
    (
        format!("{name} (features: {})", features.join(",")),
        run,
        env,
    )
}

pub(crate) fn doc_features(
    cargo: &str,
    features: &[String],
) -> (String, String, Vec<(String, Value)>) {
    (
        format!("Documentation (features: {})", features.join(",")),
        format!("{DOC_RUN}{cargo}{}", feature_flag(features)),
        vec![],
    )
}
