// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use std::path::{Path, PathBuf};

fn internal(problem: &str) -> OrchestratorError {
    // Local copy of `crate::internal::internal`: identical value, so this
    // module stays includable by path from integration tests (the test crate
    // has no `crate::internal` module to resolve that import from).
    OrchestratorError::Internal {
        problem: problem.to_owned(),
    }
}

pub(crate) const OWNED_ROOT_DIR: &str = "velnor-gc1";
pub(crate) const TRANSPORT_DIR: &str = "transport";
pub(crate) const SOURCE_DIR: &str = "sources";
pub(crate) const TOOL_DIR: &str = "tools";
pub(crate) const OBSERVATION_DIR: &str = "observations";
pub(crate) const CARGO_DIR: &str = "cargo";
pub(crate) const CHECK_HOME_PREFIX: &str = "velnor-check-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedExecutionRoots {
    pub(crate) owned_root: PathBuf,
    pub(crate) transport_root: PathBuf,
    pub(crate) source_root: PathBuf,
    pub(crate) tool_root: PathBuf,
    pub(crate) cargo_home: PathBuf,
    pub(crate) rustup_home: PathBuf,
    pub(crate) observation_root: PathBuf,
}

impl OwnedExecutionRoots {
    pub(crate) fn all(&self) -> [&Path; 7] {
        [
            self.owned_root.as_path(),
            self.transport_root.as_path(),
            self.source_root.as_path(),
            self.tool_root.as_path(),
            self.cargo_home.as_path(),
            self.rustup_home.as_path(),
            self.observation_root.as_path(),
        ]
    }
}

pub(crate) fn resolve_roots(temp: &Path) -> Result<OwnedExecutionRoots, OrchestratorError> {
    if !temp.is_absolute() {
        return Err(internal("execution_temp_not_absolute"));
    }
    let owned_root = temp.join(OWNED_ROOT_DIR);
    Ok(OwnedExecutionRoots {
        transport_root: owned_root.join(TRANSPORT_DIR),
        source_root: owned_root.join(SOURCE_DIR),
        tool_root: owned_root.join(TOOL_DIR),
        cargo_home: owned_root.join(CARGO_DIR),
        rustup_home: owned_root.join(velnor_actions_mise::checks::RUSTUP_HOME_SUFFIX),
        observation_root: owned_root.join(OBSERVATION_DIR),
        owned_root,
    })
}

pub(crate) fn ensure_private_placement(
    workspace_root: &Path,
    temp: &Path,
    home: &Path,
) -> Result<(), OrchestratorError> {
    if !workspace_root.is_absolute() {
        return Err(internal("execution_workspace_not_absolute"));
    }
    if temp == workspace_root
        || temp.starts_with(workspace_root)
        || workspace_root.starts_with(temp)
    {
        return Err(internal("execution_temp_inside_checkout"));
    }
    let roots = resolve_roots(temp)?;
    let all = roots.all();
    for root in all {
        if root.starts_with(workspace_root) || workspace_root.starts_with(root) {
            return Err(internal("execution_root_inside_checkout"));
        }
    }
    // `all()[0]` is the owned parent that contains the other six roots by
    // design, so pairwise disjointness only applies to the six siblings.
    for (index, first) in all.iter().enumerate().skip(1) {
        for second in &all[index + 1..] {
            if first == second || first.starts_with(second) || second.starts_with(first) {
                return Err(internal("execution_roots_overlap"));
            }
        }
    }
    if !home.is_absolute() || !home.starts_with(temp) || !is_private_check_home(home) {
        return Err(internal("execution_home_not_private"));
    }
    if all
        .iter()
        .any(|root| home == *root || home.starts_with(root))
    {
        return Err(internal("execution_home_inside_shared_roots"));
    }
    Ok(())
}

pub(crate) fn is_private_check_home(home: &Path) -> bool {
    home.is_absolute()
        && home
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(CHECK_HOME_PREFIX))
}

pub(crate) fn discard_private_home(home: &Path) {
    if !is_private_check_home(home) {
        eprintln!("velnor_check_cleanup_refused:shared_home");
        return;
    }
    if let Err(error) = std::fs::remove_dir_all(home) {
        eprintln!("velnor_check_cleanup_failed:{:?}", error.kind());
    }
}
