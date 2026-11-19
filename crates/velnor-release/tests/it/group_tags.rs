// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The repository's own `release-plz.toml` follows the group-prefixed tag
//! scheme of #1429: `tools-v` for velnor-ci and velnor-gen, `runner-v` for the
//! runner, and never a bare `v<version>`.

use std::collections::{BTreeMap, BTreeSet};

use crate::fixture::TestResult;

/// The `[[package]]` entries of the repository's `release-plz.toml`, by name,
/// read as plain TOML.
fn entries() -> Result<BTreeMap<String, toml::Table>, Box<dyn std::error::Error>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../release-plz.toml");
    let document: toml::Table = std::fs::read_to_string(path)?.parse()?;
    let packages = document
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or("no [[package]] entries")?;
    let mut by_name = BTreeMap::new();
    for package in packages {
        let table = package
            .as_table()
            .ok_or("a package entry is not a table")?
            .clone();
        let name = table
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or("a package has no name")?
            .to_owned();
        by_name.insert(name, table);
    }
    Ok(by_name)
}

fn str_field<'a>(entry: &'a toml::Table, key: &str) -> Option<&'a str> {
    entry.get(key).and_then(toml::Value::as_str)
}

/// The names of the packages in `group`, and the one tag pattern they share.
fn group(name: &str) -> Result<(BTreeSet<String>, BTreeSet<String>), Box<dyn std::error::Error>> {
    let mut members = BTreeSet::new();
    let mut patterns = BTreeSet::new();
    for (package, entry) in entries()? {
        if str_field(&entry, "version_group") == Some(name) {
            members.insert(package);
            patterns.insert(
                str_field(&entry, "git_tag_name")
                    .ok_or("a grouped package has no git_tag_name")?
                    .to_owned(),
            );
        }
    }
    Ok((members, patterns))
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn the_tools_group_is_velnor_ci_and_velnor_gen_with_tools_v() -> TestResult {
    let (members, patterns) = group("tools")?;
    assert_eq!(members, set(&["velnor-ci", "velnor-gen"]));
    assert_eq!(patterns, set(&["tools-v{{ version }}"]));
    Ok(())
}

#[test]
fn the_runner_group_tags_with_runner_v() -> TestResult {
    let (members, patterns) = group("runner")?;
    assert!(members.contains("velnor-runner-cli"));
    assert_eq!(patterns, set(&["runner-v{{ version }}"]));
    Ok(())
}

/// A group has exactly one lead. It owns the tag and the GitHub release; the
/// other members keep its `git_tag_name` and turn their own tag off.
#[test]
fn every_group_has_one_lead_that_owns_the_tag() -> TestResult {
    for name in ["tools", "runner"] {
        let mut leads = 0;
        for (package, entry) in entries()? {
            if str_field(&entry, "version_group") != Some(name) {
                continue;
            }
            let tag_off = entry.get("git_tag_enable").and_then(toml::Value::as_bool) == Some(false);
            if !tag_off {
                leads += 1;
            }
            assert_eq!(
                entry.get("release").and_then(toml::Value::as_bool),
                Some(true),
                "{package}: a group member must release"
            );
        }
        assert_eq!(leads, 1, "group {name}: expected exactly one lead");
    }
    Ok(())
}

/// No package tags with a bare `v<version>`, and the old engine
/// (`velnor-actions-*`, being deleted) neither releases nor joins a group.
#[test]
fn no_bare_v_tag_and_no_engine_group() -> TestResult {
    for (package, entry) in entries()? {
        if let Some(pattern) = str_field(&entry, "git_tag_name") {
            assert!(
                !pattern.starts_with("v{{"),
                "{package}: bare tag pattern {pattern:?}"
            );
        }
        if package.starts_with("velnor-actions-") {
            assert_ne!(
                entry.get("release").and_then(toml::Value::as_bool),
                Some(true),
                "{package}: the engine must not release"
            );
            assert!(
                entry.get("version_group").is_none(),
                "{package}: the engine has no group"
            );
        }
    }
    Ok(())
}
