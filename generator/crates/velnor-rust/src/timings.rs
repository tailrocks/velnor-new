// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use velnor_model::{JobId, RepoPath, RootSlug, RustRoot, Timings};

use crate::error::{DetectError, Reason};
use crate::plan::{DEFAULT_GROUP_JOB, LINT_JOB, RUST_JOB, TEST_JOB};

pub const TIMINGS_PATH: RepoPath = RepoPath::from_static(".velnor/rust-ci-timings.json");

#[derive(Deserialize, Serialize)]
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

#[must_use]
pub fn render_timings(timings: &BTreeMap<RepoPath, Timings>) -> String {
    let raw: BTreeMap<&str, RawTimings> = timings
        .iter()
        .map(|(root, timings)| {
            (
                root.as_str(),
                RawTimings {
                    lint_build: timings.lint_build_seconds,
                    test_build: timings.test_build_seconds,
                    tests: timings.test_seconds,
                    binaries: timings.test_binary_seconds.clone(),
                },
            )
        })
        .collect();
    let mut text = serde_json::to_string_pretty(&raw).unwrap_or_default();
    text.push('\n');
    text
}

#[must_use]
pub fn root_job_ids(manifest: &RepoPath, several_roots: bool) -> [JobId; 4] {
    [RUST_JOB, LINT_JOB, TEST_JOB, DEFAULT_GROUP_JOB].map(|id| {
        if several_roots {
            id.suffixed(RootSlug::of(manifest).as_str())
        } else {
            id
        }
    })
}

pub(crate) fn usable_timings(
    bytes: Option<&Vec<u8>>,
    roots: &[RustRoot],
) -> (BTreeMap<RepoPath, Timings>, Vec<String>) {
    let Some(bytes) = bytes else {
        return (BTreeMap::new(), Vec::new());
    };
    let parsed = match parse_timings(bytes) {
        Ok(parsed) => parsed,
        Err(error) => return (BTreeMap::new(), vec![format!("{error}; ignored")]),
    };
    let (known, gone): (BTreeMap<_, _>, BTreeMap<_, _>) = parsed
        .into_iter()
        .partition(|(root, _)| roots.iter().any(|known| known.manifest == *root));
    let notes = gone
        .into_keys()
        .map(|root| {
            format!(
                "{}; entry ignored",
                DetectError::invalid(&TIMINGS_PATH, Reason::UnknownRoot(root))
            )
        })
        .collect();
    (known, notes)
}
