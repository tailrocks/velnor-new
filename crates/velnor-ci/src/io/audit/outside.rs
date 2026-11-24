// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::REBUILD;
use crate::error::CiError;

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
        None => format!(
            "This usually means an earlier `velnor-ci inputs build` step in this job failed (failed commands record no receipts); fix that step first. Otherwise prepare inventory first with {REBUILD}, preserving this build's feature, target and profile options"
        ),
    };
    CiError::new(format!(
        "untrusted build outputs include packages outside this command's explicit selection: {}. {cause}",
        names.join(", ")
    ))
}
