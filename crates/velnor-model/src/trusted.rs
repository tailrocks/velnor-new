// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::RunnerLabel;

pub const TRUSTED_EVENT: &str = "(github.event_name == 'schedule' || ((github.event_name == 'push' || github.event_name == 'workflow_dispatch') && github.ref == format('refs/heads/{0}', github.event.repository.default_branch)))";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedRunners {
    pairs: Vec<(RunnerLabel, RunnerLabel)>,
}

impl TrustedRunners {
    #[must_use]
    pub fn new(pairs: Vec<(RunnerLabel, RunnerLabel)>) -> Self {
        Self { pairs }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    #[must_use]
    pub fn for_runner(&self, runner: &RunnerLabel) -> Option<&RunnerLabel> {
        self.pairs
            .iter()
            .find(|(label, _)| label.as_str().eq_ignore_ascii_case(runner.as_str()))
            .map(|(_, trusted)| trusted)
    }
}
