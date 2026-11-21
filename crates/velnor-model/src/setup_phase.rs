// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::repo_config::TestSetup;

impl TestSetup {
    #[must_use]
    pub fn before_build(&self) -> bool {
        self.phase == Some(SetupPhase::Build)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupPhase {
    Test,
    Build,
}

/// Where a step sits against the `[[test_setup]]` steps of its job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupOrder {
    /// Not gated by the setup steps.
    Unordered,
    /// Skipped when a setup step before it failed or was cancelled.
    AfterSetup,
}
