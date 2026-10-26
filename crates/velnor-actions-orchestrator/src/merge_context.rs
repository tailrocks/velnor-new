// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct MergeContext {
    repository_present: bool,
    workspace: Option<PathBuf>,
}

impl MergeContext {
    #[must_use]
    pub fn detached() -> Self {
        Self {
            repository_present: false,
            workspace: None,
        }
    }

    #[must_use]
    pub fn required_workspace(workspace: Option<PathBuf>) -> Self {
        Self {
            repository_present: true,
            workspace,
        }
    }

    pub(super) fn capture_ambient() -> Self {
        Self {
            repository_present: std::env::var_os("GITHUB_REPOSITORY").is_some(),
            workspace: std::env::var_os("GITHUB_WORKSPACE").map(PathBuf::from),
        }
    }

    pub(super) const fn repository_present(&self) -> bool {
        self.repository_present
    }

    pub(super) fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }
}
