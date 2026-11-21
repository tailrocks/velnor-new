// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use velnor_rust::KEYS;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn documented_keys(page: &str) -> Vec<String> {
    page.lines()
        .skip_while(|line| *line != "## Settings")
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .filter(|line| line.starts_with("| `"))
        .filter_map(|line| line.split('|').nth(1))
        .flat_map(|cell| cell.split('`').skip(1).step_by(2))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_settings_table_of_the_rust_ci_page_lists_exactly_the_keys_the_config_accepts() -> TestResult
{
    let page =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/rust-ci.md"))?;
    let documented = documented_keys(&page);
    let documented_set: BTreeSet<&str> = documented.iter().map(String::as_str).collect();
    let accepted: BTreeSet<&str> = KEYS.iter().copied().collect();
    assert_eq!(
        documented.len(),
        documented_set.len(),
        "a key is listed twice"
    );
    let missing: Vec<_> = accepted.difference(&documented_set).collect();
    let extra: Vec<_> = documented_set.difference(&accepted).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "missing from docs/rust-ci.md: {missing:?}; not accepted by the config: {extra:?}"
    );
    Ok(())
}
