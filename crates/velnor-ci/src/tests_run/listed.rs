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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listing {
    pub(crate) packages: BTreeMap<String, u64>,
    pub(crate) ignored: u64,
}

#[must_use]
pub(crate) fn runnable_matches(bytes: &[u8]) -> Option<Listing> {
    let listed = parse(bytes)?;
    let mut packages = BTreeMap::new();
    let mut ignored = 0u64;
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
                    if filter.reason.as_deref() == Some("ignored") {
                        ignored = ignored.checked_add(1)?;
                    }
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
    Some(Listing { packages, ignored })
}

fn parse(bytes: &[u8]) -> Option<Listed> {
    serde_json::from_slice(bytes).ok().or_else(|| {
        let last = bytes
            .split(|byte| *byte == b'\n')
            .rfind(|line| !line.trim_ascii().is_empty())?;
        serde_json::from_slice(last).ok()
    })
}

fn add(into: &mut BTreeMap<String, u64>, name: &str, count: u64) -> Option<()> {
    if count > 0 {
        let total = into.get(name).copied().unwrap_or(0_u64);
        into.insert(name.to_owned(), total.checked_add(count)?);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::runnable_matches;

    const LISTING: &str = r#"{"rust-build-meta":{},"test-count":1,"rust-suites":{"a::lib":{"package-name":"a","testcases":{"t":{"filter-match":{"status":"matches"}}}}}}"#;

    #[test]
    fn listing_alone_parses() {
        let listing = runnable_matches(LISTING.as_bytes()).expect("parses");
        assert_eq!(listing.packages.get("a"), Some(&1));
    }

    #[test]
    fn listing_after_cargo_json_messages_parses() {
        let stdout = format!(
            "{{\"reason\":\"compiler-artifact\",\"package_id\":\"a\"}}\n{{\"reason\":\"build-finished\",\"success\":true}}\n{LISTING}\n"
        );
        let listing = runnable_matches(stdout.as_bytes()).expect("parses");
        assert_eq!(listing.packages.get("a"), Some(&1));
    }

    #[test]
    fn cargo_json_without_a_listing_fails_closed() {
        let stdout = "{\"reason\":\"build-finished\",\"success\":true}\n";
        assert_eq!(runnable_matches(stdout.as_bytes()), None);
        assert_eq!(runnable_matches(b""), None);
    }
}
