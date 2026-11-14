// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;
use velnor_model::{RepoPath, Timings};

use crate::error::{DetectError, Reason};

pub const TIMINGS_PATH: RepoPath = RepoPath::from_static(".velnor/rust-ci-timings.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTimings {
    #[serde(rename = "lint_build_seconds")]
    lint_build: u64,
    #[serde(rename = "test_build_seconds")]
    test_build: u64,
    #[serde(rename = "test_seconds")]
    tests: u64,
    #[serde(rename = "test_binary_seconds", default)]
    binaries: BTreeMap<String, u64>,
}

pub(crate) fn parse_timings(bytes: &[u8]) -> Result<BTreeMap<RepoPath, Timings>, DetectError> {
    let invalid = |reason| DetectError::invalid(&TIMINGS_PATH, reason);
    let raw: BTreeMap<String, RawTimings> = serde_json::from_slice(bytes)
        .map_err(|error| invalid(Reason::Syntax(error.to_string())))?;
    raw.into_iter()
        .map(|(root, timings)| {
            let root = RepoPath::new(root.clone()).map_err(|_| {
                invalid(Reason::InvalidValue {
                    key: "root",
                    value: root,
                })
            })?;
            Ok((
                root,
                Timings {
                    lint_build_seconds: timings.lint_build,
                    test_build_seconds: timings.test_build,
                    test_seconds: timings.tests,
                    test_binary_seconds: timings.binaries,
                },
            ))
        })
        .collect()
}
