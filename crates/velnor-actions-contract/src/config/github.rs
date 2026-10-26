// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnmanagedPolicy {
    #[default]
    Allow,

    Warn,

    Deny,
}

impl UnmanagedPolicy {
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubConfig {
    #[serde(default, skip_serializing_if = "UnmanagedPolicy::is_default")]
    pub unmanaged: UnmanagedPolicy,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
}

impl GithubConfig {
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.unmanaged.is_default() && self.allow.is_empty()
    }

    #[must_use]
    pub fn is_allowed(&self, path: &str) -> bool {
        self.allow.iter().any(|entry| {
            path == entry
                || path
                    .strip_prefix(entry.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let mut seen = std::collections::BTreeSet::new();
        for entry in &self.allow {
            let bad = |problem: &str| {
                ContractError::config(file, "github.allow", format!("{problem}:{entry}"))
            };
            let Some(rest) = entry.strip_prefix(".github/") else {
                return Err(bad("allow_outside_github"));
            };
            if rest.is_empty()
                || rest
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || rest.contains('\\')
            {
                return Err(bad("allow_path_not_normalized"));
            }
            if rest == "workflows" || rest.starts_with("workflows/") {
                return Err(bad("allow_workflow_use_preserve_workflows"));
            }
            if !seen.insert(entry.as_str()) {
                return Err(bad("duplicate_allow"));
            }
        }
        Ok(())
    }
}
