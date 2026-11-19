// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::RunnerLabel;

/// The runner labels a repository declared `hosted = true`.
///
/// A job on any other label is self-hosted: the generator fences it from pull requests
/// of forks. The decision never reads the spelling of a label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostedRunners {
    labels: Vec<RunnerLabel>,
    key_written: bool,
}

impl HostedRunners {
    #[must_use]
    pub fn new(labels: Vec<RunnerLabel>) -> Self {
        Self {
            labels,
            key_written: false,
        }
    }

    /// Records that `rust-ci.toml` wrote a `hosted` key, which a `velnor-ci` before 0.1.99
    /// refuses to read.
    #[must_use]
    pub fn with_key_written(self, key_written: bool) -> Self {
        Self {
            key_written,
            ..self
        }
    }

    #[must_use]
    pub fn key_written(&self) -> bool {
        self.key_written
    }

    /// True when `label` was declared hosted. GitHub matches labels without regard to case.
    #[must_use]
    pub fn contains(&self, label: &RunnerLabel) -> bool {
        self.labels
            .iter()
            .any(|hosted| hosted.as_str().eq_ignore_ascii_case(label.as_str()))
    }
}
