// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::RepoPath;

/// One `[[check_configurations]]` entry: a `velnor-ci check-configurations` step in the jobs of
/// `root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckConfigurations {
    pub root: Option<RepoPath>,
    /// The JSON file of named feature configurations, relative to `root`. Without it the step
    /// checks the default configuration.
    pub configurations: Option<RepoPath>,
}

impl CheckConfigurations {
    /// The `run` value of the step.
    #[must_use]
    pub fn command(&self) -> String {
        match &self.configurations {
            Some(file) => format!("velnor-ci check-configurations --configurations {file}"),
            None => "velnor-ci check-configurations".to_owned(),
        }
    }
}
