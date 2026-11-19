// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::facts::TestResult;
use crate::golden::{sample_dirs, samples};

fn names(text: &str, marker: &str) -> BTreeSet<String> {
    text.match_indices(marker)
        .map(|(at, _)| {
            text[at + marker.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                .collect()
        })
        .collect()
}

fn workflows(directory: &Path, found: &mut Vec<std::path::PathBuf>) -> TestResult {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            workflows(&path, found)?;
        } else if path.extension().is_some_and(|ext| ext == "yml") {
            found.push(path);
        }
    }
    Ok(())
}

#[test]
fn no_rendered_sample_workflow_has_an_anchor_nobody_aliases() -> TestResult {
    let mut files = Vec::new();
    for sample in sample_dirs(&samples())? {
        workflows(&sample, &mut files)?;
    }
    assert!(!files.is_empty(), "no sample workflow recorded");
    let mut unused = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file)?;
        let aliases = names(&text, ": *");
        for anchor in names(&text, ": &") {
            if !aliases.contains(&anchor) {
                unused.push(format!("{}: &{anchor}", file.display()));
            }
        }
    }
    assert!(unused.is_empty(), "unused anchors: {unused:#?}");
    Ok(())
}
