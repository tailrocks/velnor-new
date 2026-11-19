// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

pub const DEFAULT_RUNNER_WORKSPACE_ROOT: &str = "crates/velnor-runner";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerWorkspaceConfig {
    #[serde(default = "default_root")]
    pub root: String,
}

fn default_root() -> String {
    DEFAULT_RUNNER_WORKSPACE_ROOT.to_owned()
}

impl Default for RunnerWorkspaceConfig {
    fn default() -> Self {
        Self {
            root: default_root(),
        }
    }
}

impl RunnerWorkspaceConfig {
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.root == DEFAULT_RUNNER_WORKSPACE_ROOT
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let root = self.root.as_str();
        let malformed = root.starts_with('/')
            || root.ends_with('/')
            || root.contains("//")
            || root
                .split('/')
                .any(|segment| segment == "." || segment == "..")
            || !root.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_')
            });
        if malformed {
            return Err(ContractError::config(
                file,
                "runner_workspace.root",
                format!("malformed_root:{root}"),
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn manifest_path(&self) -> String {
        self.join("Cargo.toml")
    }

    #[must_use]
    pub fn lock_path(&self) -> String {
        self.join("Cargo.lock")
    }

    #[must_use]
    pub fn target_dir(&self) -> String {
        self.join("target")
    }

    #[must_use]
    pub fn fetch_roots(&self) -> Vec<String> {
        if self.root.is_empty() {
            vec![String::new()]
        } else {
            vec![String::new(), self.root.clone()]
        }
    }

    #[must_use]
    pub fn manifest_paths(&self) -> Vec<String> {
        let mut paths = vec!["Cargo.toml".to_owned()];
        if !self.root.is_empty() {
            paths.push(self.manifest_path());
        }
        paths
    }

    fn join(&self, leaf: &str) -> String {
        if self.root.is_empty() {
            leaf.to_owned()
        } else {
            format!("{}/{leaf}", self.root)
        }
    }
}
