// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_rust::detect;

use crate::facts::TestResult;
use crate::golden::{load, samples};

#[test]
fn the_recorded_repository_sample_has_a_root_per_workspace_with_the_members_cargo_reports()
-> TestResult {
    let sample = samples().join("velnor-new");
    let facts = detect(&load(&sample)?)?;
    let mut reported = Vec::new();
    for entry in walk(&sample.join("cargo-metadata"))? {
        let json: serde_json::Value = serde_json::from_slice(&fs::read(&entry)?)?;
        let relative = entry
            .strip_prefix(sample.join("cargo-metadata"))?
            .to_str()
            .ok_or("path is not UTF-8")?
            .trim_end_matches(".json")
            .to_owned();
        let packages = json["packages"].as_array().ok_or("no packages")?;
        let tested = packages
            .iter()
            .filter(|package| {
                package["targets"]
                    .as_array()
                    .is_some_and(|targets| targets.iter().any(|t| t["test"] == true))
            })
            .count();
        reported.push((relative, packages.len(), tested));
    }
    reported.sort();
    let found: Vec<(String, usize, usize)> = facts
        .rust_roots
        .iter()
        .map(|root| {
            let tested = root.members.iter().filter(|m| m.has_test_target()).count();
            (root.manifest.to_string(), root.members.len(), tested)
        })
        .collect();
    let mut expected = reported;
    expected.sort();
    let mut found_sorted = found.clone();
    found_sorted.sort();
    assert_eq!(found_sorted, expected);
    assert_eq!(
        found
            .iter()
            .map(|(manifest, _, _)| manifest.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Cargo.toml",
            "ci-helper/Cargo.toml",
            "crates/velnor-runner/Cargo.toml",
            "generator/Cargo.toml",
            "local/Cargo.toml",
        ]
    );
    assert_eq!(
        facts.toolchain.map(|toolchain| toolchain.channel),
        Some("1.99.1".to_owned())
    );
    assert!(
        facts
            .tools
            .tools
            .contains_key("aqua:nextest-rs/nextest/cargo-nextest")
    );
    Ok(())
}

fn walk(directory: &Path) -> Result<Vec<std::path::PathBuf>, Box<dyn Error>> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                found.push(entry.path());
            }
        }
    }
    Ok(found)
}
