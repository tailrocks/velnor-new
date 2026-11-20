// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::repo_config::TestSetup;

impl TestSetup {
    /// Whether the step runs before the first cargo step that compiles, not only before the tests.
    #[must_use]
    pub fn before_build(&self) -> bool {
        self.phase == Some(SetupPhase::Build)
    }
}

/// How early a `[[test_setup]]` step runs in the jobs of its root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupPhase {
    /// Before the first test step.
    Test,
    /// Before the first cargo step that compiles or runs tests, so a build script can read it.
    Build,
}
