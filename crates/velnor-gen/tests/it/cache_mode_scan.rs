// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Which generated workflow files may carry a `cache-mode` key (see `fixed_uses`).

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn Error>>;

fn yaml_files(dir: &Path, found: &mut Vec<PathBuf>) -> TestResult {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            yaml_files(&path, found)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "yml" || extension == "yaml")
        {
            found.push(path);
        }
    }
    Ok(())
}

const JOB_CACHE_MODES: &[&str] = &[
    "    cache-mode: read",
    "    cache-mode: write",
    "    cache-mode: write-only",
    "    cache-mode: none",
];

pub(crate) fn without_github_mode(text: &str) -> String {
    text.lines()
        .filter(|line| !JOB_CACHE_MODES.contains(line))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("github-cache-mode", "")
}

pub(crate) fn carries_cache_mode(text: &str) -> bool {
    without_github_mode(text).contains("cache-mode")
}

/// Only velnor-gen output counts: `generator/samples/<sample>/expected/`. The `view/` trees
/// are inputs, copies of hand-maintained repo files such as `.github/workflows/ci.yml`.
pub(crate) fn rendered_samples(samples: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    for sample in fs::read_dir(samples)? {
        let expected = sample?.path().join("expected");
        if expected.is_dir() {
            yaml_files(&expected, &mut files)?;
        }
    }
    Ok(files)
}
