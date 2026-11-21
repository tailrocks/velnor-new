// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use crate::facts::TestResult;

const SOURCES: [&str; 4] = [
    "velnor-gen/src",
    "velnor-model/src",
    "velnor-render/src",
    "velnor-rust/src",
];

fn crates() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

fn gates_in(text: &str, found: &mut Vec<u64>) {
    for (at, _) in text.match_indices("(0, 1, ") {
        let digits: String = text[at + 7..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if text[at + 7 + digits.len()..].starts_with(')')
            && let Ok(patch) = digits.parse()
        {
            found.push(patch);
        }
    }
}

fn gates_under(directory: &Path, found: &mut Vec<u64>) -> TestResult {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            gates_under(&path, found)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            gates_in(&fs::read_to_string(&path)?, found);
        }
    }
    Ok(())
}

fn released_patch() -> Result<u64, Box<dyn std::error::Error>> {
    let manifest = fs::read_to_string(crates().join("velnor-ci/Cargo.toml"))?;
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \"0.1."))
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or("no velnor-ci version")?
        .parse()
        .map_err(Into::into)
}

pub(crate) fn newest_helper() -> Result<String, Box<dyn std::error::Error>> {
    let mut found = vec![released_patch()?];
    for source in SOURCES {
        gates_under(&crates().join(source), &mut found)?;
    }
    let patch = found.into_iter().max().ok_or("no helper version")?;
    Ok(format!("0.1.{patch}"))
}

#[test]
fn the_newest_helper_is_at_least_the_helper_this_checkout_releases() -> TestResult {
    let newest = newest_helper()?;
    let patch: u64 = newest.rsplit('.').next().ok_or("no patch")?.parse()?;
    assert!(patch >= released_patch()?, "{newest}");
    Ok(())
}

#[test]
fn a_gate_is_read_from_its_tuple() {
    let mut found = Vec::new();
    gates_in(
        "at_least((0, 1, 7)) (0, 1, 120) (0, 1, x) (0, 2, 9)",
        &mut found,
    );
    assert_eq!(found, [7, 120]);
}
