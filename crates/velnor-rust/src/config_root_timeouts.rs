// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{RepoPath, RustCiConfig, RustRoot};

use crate::config::{CONFIG_PATH, TIMEOUT_RANGE};
use crate::error::{DetectError, Reason};

/// Parses `[root_timeouts]`: each key is the folder of a root (`.` for the repository root) and
/// each value a time limit in minutes.
pub(crate) fn parse(raw: BTreeMap<String, u16>) -> Result<BTreeMap<Option<RepoPath>, u16>, Reason> {
    let mut timeouts = BTreeMap::new();
    for (folder, minutes) in raw {
        let root = match folder.as_str() {
            "." => None,
            directory => Some(RepoPath::new(directory.trim_end_matches('/')).map_err(|_| {
                Reason::InvalidValue {
                    key: "root_timeouts",
                    value: folder.clone(),
                }
            })?),
        };
        if !TIMEOUT_RANGE.contains(&minutes) {
            return Err(Reason::OutOfRange {
                key: "root_timeouts",
                value: minutes,
                min: *TIMEOUT_RANGE.start(),
                max: *TIMEOUT_RANGE.end(),
            });
        }
        // `crates/x` and `crates/x/` name the same root, so either one may come second.
        if timeouts.insert(root, minutes).is_some() {
            return Err(Reason::InvalidValue {
                key: "root_timeouts",
                value: folder,
            });
        }
    }
    Ok(timeouts)
}

/// Every key must name a root of this repository, so a misspelt folder fails instead of
/// silently keeping the repository limit.
pub(crate) fn check(config: &RustCiConfig, rust_roots: &[RustRoot]) -> Result<(), DetectError> {
    let roots: BTreeSet<Option<RepoPath>> = rust_roots
        .iter()
        .map(|root| root.manifest.parent())
        .collect();
    match config
        .root_timeouts
        .keys()
        .find(|root| !roots.contains(root))
    {
        None => Ok(()),
        Some(root) => Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::UnknownRootTimeoutRoot(
                root.as_ref()
                    .map_or_else(|| ".".to_owned(), ToString::to_string),
            ),
        )),
    }
}
