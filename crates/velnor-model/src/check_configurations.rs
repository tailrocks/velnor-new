// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::RepoPath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckConfigurations {
    pub root: Option<RepoPath>,
    pub configurations: Option<RepoPath>,
}

impl CheckConfigurations {
    #[must_use]
    pub fn command(&self) -> String {
        match &self.configurations {
            Some(file) => format!("velnor-ci check-configurations --configurations={file}"),
            None => "velnor-ci check-configurations".to_owned(),
        }
    }

    #[must_use]
    pub fn file(&self) -> Option<RepoPath> {
        let file = self.configurations.as_ref()?;
        Some(match &self.root {
            Some(root) => root.child(file.as_str()),
            None => file.clone(),
        })
    }
}
