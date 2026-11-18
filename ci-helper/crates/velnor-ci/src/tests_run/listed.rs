// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;
use velnor_rust::package_token;

const NOT_RUNNABLE: [&str; 2] = ["ignored", "not-benchmark"];

#[derive(Deserialize)]
pub(super) struct Listed {
    #[serde(rename = "rust-suites")]
    pub(super) rust_suites: BTreeMap<String, Suite>,
}

#[derive(Deserialize)]
pub(super) struct Suite {
    #[serde(rename = "package-name")]
    pub(super) package_name: String,
    pub(super) testcases: BTreeMap<String, Case>,
}

#[derive(Deserialize)]
pub(super) struct Case {
    #[serde(rename = "filter-match")]
    pub(super) filter_match: FilterMatch,
}

#[derive(Deserialize)]
pub(super) struct FilterMatch {
    pub(super) status: String,
    #[serde(default)]
    pub(super) reason: Option<String>,
}

#[must_use]
pub(crate) fn runnable_matches(bytes: &[u8]) -> Option<BTreeMap<String, u64>> {
    let listed: Listed = serde_json::from_slice(bytes).ok()?;
    let mut packages = BTreeMap::new();
    for suite in listed.rust_suites.into_values() {
        if !package_token(&suite.package_name) {
            return None;
        }
        let mut runnable = 0u64;
        let mut excluded = 0u64;
        for case in suite.testcases.into_values() {
            let filter = case.filter_match;
            match filter.status.as_str() {
                "matches" => runnable = runnable.checked_add(1)?,
                "mismatch" => {
                    if !filter
                        .reason
                        .as_deref()
                        .is_some_and(|reason| NOT_RUNNABLE.contains(&reason))
                    {
                        excluded = excluded.checked_add(1)?;
                    }
                }
                _ => return None,
            }
        }
        add(&mut packages, &suite.package_name, runnable)?;
        add(&mut packages, &suite.package_name, excluded)?;
    }
    Some(packages)
}

fn add(into: &mut BTreeMap<String, u64>, name: &str, count: u64) -> Option<()> {
    if count > 0 {
        let total = into.get(name).copied().unwrap_or(0_u64);
        into.insert(name.to_owned(), total.checked_add(count)?);
    }
    Some(())
}
