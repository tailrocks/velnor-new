// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;
use velnor_rust::package_token;

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
}

#[must_use]
pub(crate) fn runnable_matches(bytes: &[u8]) -> Option<BTreeMap<String, u64>> {
    let listed: Listed = serde_json::from_slice(bytes).ok()?;
    let mut packages = BTreeMap::new();
    let mut excluded = BTreeMap::new();
    for suite in listed.rust_suites.into_values() {
        if !package_token(&suite.package_name) {
            return None;
        }
        let mut matched = 0u64;
        let mut mismatched = 0u64;
        for case in suite.testcases.into_values() {
            match case.filter_match.status.as_str() {
                "matches" => matched = matched.checked_add(1)?,
                "mismatch" => mismatched = mismatched.checked_add(1)?,
                _ => return None,
            }
        }
        add(&mut packages, &suite.package_name, matched)?;
        add(&mut excluded, &suite.package_name, mismatched)?;
    }
    Some(if packages.is_empty() {
        excluded
    } else {
        packages
    })
}

fn add(into: &mut BTreeMap<String, u64>, name: &str, count: u64) -> Option<()> {
    if count > 0 {
        let total = into.get(name).copied().unwrap_or(0_u64);
        into.insert(name.to_owned(), total.checked_add(count)?);
    }
    Some(())
}
