// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{FilterRef, LaneFeatures, PackageName, TEST_STEP};

use crate::cargo::{WORKSPACE, clippy, lane_flags, nextest_run, without_binaries};
use crate::rules::Value;

/// The default step may select nothing, because the plan narrows it to the
/// changed packages. A feature lane names its tests, so none matching is a
/// broken filter and must not pass.
pub(crate) const NO_TESTS_PASS: &str = "pass";
pub(crate) const NO_TESTS_FAIL: &str = "fail";

pub(crate) fn nextest_features(
    nextest: &str,
    features: &LaneFeatures,
    filter: &FilterRef,
    ci_profile: bool,
) -> (String, String, Vec<(String, Value)>) {
    let (_, run, env) = nextest_run(
        &format!("--workspace --locked{nextest}{}", lane_flags(features)),
        filter,
        ci_profile,
        NO_TESTS_FAIL,
        "",
    );
    (format!("{TEST_STEP} ({})", features.label()), run, env)
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub(crate) fn lane_guard(
    features: &LaneFeatures,
    filter: &str,
    except: &[String],
) -> (String, String, Vec<(String, Value)>) {
    let run = except.iter().fold(
        format!(
            "velnor-ci lane-guard{} --filter {}",
            lane_flags(features),
            quoted(filter)
        ),
        |text, id| format!("{text} --except-binary {}", quoted(id)),
    );
    (
        format!("Test lane guard ({})", features.label()),
        run,
        vec![],
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
    features: &LaneFeatures,
    packages: &[PackageName],
) -> (String, String, Vec<(String, Value)>) {
    let (name, run, env) = clippy(
        &format!("{cargo}{}", lane_flags(features)),
        &[],
        "",
        &package_scope(packages),
    );
    (format!("{name} ({})", features.label()), run, env)
}

pub(crate) fn doc_features(
    cargo: &str,
    features: &LaneFeatures,
    packages: &[PackageName],
) -> (String, String, Vec<(String, Value)>) {
    (
        format!("Documentation ({})", features.label()),
        format!(
            "{}{cargo}{}",
            doc_run(&package_scope(packages)),
            lane_flags(features)
        ),
        vec![],
    )
}

pub(crate) fn doc_run(scope: &str) -> String {
    format!(
        "cargo doc {scope} --no-deps --locked --config 'build.rustdocflags=[\"-D\",\"warnings\"]'"
    )
}

pub(crate) fn package_scope(packages: &[PackageName]) -> String {
    if packages.is_empty() {
        return WORKSPACE.to_owned();
    }
    packages
        .iter()
        .map(|package| format!("-p {package}"))
        .collect::<Vec<_>>()
        .join(" ")
}
