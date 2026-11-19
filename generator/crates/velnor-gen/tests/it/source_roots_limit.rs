// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::{RepoPath, RepoView};

use crate::facts::TestResult;
use crate::pipeline::{before_deltas, view};

fn roots(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            if index == 0 {
                "Cargo.toml".to_owned()
            } else {
                format!("r{index}/Cargo.toml")
            }
        })
        .collect()
}

fn repo(count: usize) -> Result<RepoView, Box<dyn std::error::Error>> {
    let manifests = roots(count);
    let names: Vec<&str> = manifests.iter().map(String::as_str).collect();
    before_deltas(view(&names, None)?)
}

fn refusal(view: &RepoView) -> Option<String> {
    match expected_with_notes(view) {
        Err(error @ GenerateError::TooManySourceRoots(_)) => Some(error.to_string()),
        _ => None,
    }
}

#[test]
fn more_than_sixteen_roots_with_the_shared_sources_fetch_are_refused() -> TestResult {
    let message = refusal(&repo(17)?).ok_or("17 roots were not refused")?;
    assert!(message.contains("16"), "{message}");
    assert!(message.contains("17"), "{message}");
    assert!(message.contains("tailrocks/velnor-new#1362"), "{message}");
    assert!(
        message.contains("velnor-ci sources fetch --roots"),
        "{message}"
    );
    Ok(())
}

#[test]
fn sixteen_roots_generate() -> TestResult {
    expected_with_notes(&repo(16)?)?;
    Ok(())
}

#[test]
fn a_repository_that_saves_no_sources_is_not_limited_to_sixteen_roots() -> TestResult {
    let mut unsaving = repo(17)?;
    let lock = "version = 4\n\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/example/shared-lib?branch=main#0123456789abcdef\"\n";
    unsaving
        .files
        .insert(RepoPath::new("Cargo.lock")?, lock.as_bytes().to_vec());
    expected_with_notes(&unsaving)?;
    Ok(())
}
