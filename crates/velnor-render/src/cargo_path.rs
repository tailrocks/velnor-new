// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BUILD_TESTS_STEP, PackageName, RepoPath, TargetKind, TargetName};

use crate::cargo::{feature_flag, test_selector};
use crate::rules::Value;

pub(crate) fn nextest_archive(
    nextest: &str,
    file: &RepoPath,
    path: &str,
) -> (String, String, Vec<(String, Value)>) {
    (
        BUILD_TESTS_STEP.into(),
        format!(
            "cargo-nextest nextest archive{path} --workspace --locked{nextest} --archive-file '{}'",
            file.as_str().replace('\'', "'\\''")
        ),
        vec![],
    )
}

pub(crate) fn test_target(
    cargo: &str,
    package: &PackageName,
    target: &TargetName,
    kind: TargetKind,
    features: &[String],
    path: &str,
) -> (String, String, Vec<(String, Value)>) {
    (
        format!("Test {package} {target}"),
        format!(
            "cargo test{path} -p {package} --locked{cargo} {}{}",
            test_selector(kind, target),
            feature_flag(features)
        ),
        vec![],
    )
}

pub(crate) fn manifest_flag(manifest: Option<&RepoPath>) -> String {
    manifest.map_or_else(String::new, |path| {
        format!(
            " --manifest-path '{}'",
            path.as_str().replace('\'', "'\\''")
        )
    })
}
