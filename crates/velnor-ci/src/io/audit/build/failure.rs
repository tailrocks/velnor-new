// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::inventory::STORE;
use super::invocation::Selection;
use crate::error::CiError;

const MARKER: &str = "failed-inputs-build.json";

#[derive(PartialEq, Serialize, Deserialize)]
struct Run {
    id: String,
    attempt: Option<String>,
}

impl Run {
    fn current() -> Option<Self> {
        let id = std::env::var("GITHUB_RUN_ID")
            .ok()
            .filter(|id| !id.is_empty())?;
        let attempt = std::env::var("GITHUB_RUN_ATTEMPT")
            .ok()
            .filter(|attempt| !attempt.is_empty());
        Some(Self { id, attempt })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct Failure {
    command: String,
    #[serde(default)]
    selection: Option<Selection>,
    #[serde(default)]
    run: Option<Run>,
}

impl Failure {
    pub(super) fn new(command: String, selection: Option<Selection>) -> Self {
        Self {
            command,
            selection,
            run: Run::current(),
        }
    }
}

fn marker(directory: &Path) -> PathBuf {
    directory.join(STORE).join(MARKER)
}

pub(super) fn record_failure(directories: &[PathBuf], failure: &Failure) -> Result<(), CiError> {
    let bytes = serde_json::to_vec(failure).map_err(|e| CiError::new(e.to_string()))?;
    for directory in directories {
        let path = marker(directory);
        fs::create_dir_all(directory.join(STORE))
            .and_then(|()| fs::write(&path, &bytes))
            .map_err(|e| {
                CiError::new(format!(
                    "cannot record failed build in {}: {e}",
                    path.display()
                ))
            })?;
    }
    Ok(())
}

pub(super) fn clear_failure(directories: &[PathBuf]) -> Result<(), CiError> {
    for directory in directories {
        let path = marker(directory);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CiError::new(format!(
                    "cannot clear {}: {error}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn earlier_failure(
    directories: &[PathBuf],
    root: &str,
    packages: &BTreeSet<&str>,
) -> Option<String> {
    directories.iter().find_map(|directory| {
        let bytes = fs::read(marker(directory)).ok()?;
        let failure: Failure = serde_json::from_slice(&bytes).ok()?;
        if failure.run != Run::current() {
            return None;
        }
        let selection = failure.selection.as_ref()?;
        (selection.is_root(root) && packages.iter().any(|package| selection.selects(package)))
            .then_some(failure.command)
    })
}

const LISTED_PACKAGES: usize = 20;

pub(super) fn outside_selection(packages: &BTreeSet<&str>, earlier: Option<String>) -> CiError {
    let mut names: Vec<String> = packages
        .iter()
        .take(LISTED_PACKAGES)
        .map(|package| format!("{package} (build-script output)"))
        .collect();
    let more = packages.len().saturating_sub(LISTED_PACKAGES);
    if more > 0 {
        names.push(format!("and {more} more"));
    }
    let cause = match earlier {
        Some(command) => format!(
            "An earlier inputs build failed: `{command}` (failed commands record no receipts); fix that step first"
        ),
        None => "This usually means an earlier `velnor-ci inputs build` step in this job failed (failed commands record no receipts); fix that step first. Otherwise prepare inventory first with velnor-ci inputs build -- cargo build --workspace --all-targets, preserving this build's feature, target and profile options".to_owned(),
    };
    CiError::new(format!(
        "untrusted build outputs include packages outside this command's explicit selection: {}. {cause}",
        names.join(", ")
    ))
}
