// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ActionlintError;

pub const JACTIONLINT_VERSION: &str = "2.3.0";

const KNOWN_HOSTED_LABELS: [&str; 13] = [
    "macos-14",
    "macos-15",
    "macos-15-intel",
    "macos-26",
    "macos-26-intel",
    "ubuntu-22.04",
    "ubuntu-22.04-arm",
    "ubuntu-24.04",
    "ubuntu-24.04-arm",
    "ubuntu-26.04",
    "ubuntu-26.04-arm",
    "ubuntu-latest",
    "xcode-27",
];

#[must_use]
pub fn is_known_hosted_label(label: &str) -> bool {
    KNOWN_HOSTED_LABELS.contains(&label)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepSyntax {
    JobMatrix,

    NativeParallelism,

    CacheMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JactionlintCapabilities {
    job_matrix: bool,

    native_step_parallelism: bool,

    cache_mode: bool,
}

impl JactionlintCapabilities {
    #[must_use]
    pub const fn for_pinned() -> Self {
        Self {
            job_matrix: true,
            native_step_parallelism: true,
            cache_mode: true,
        }
    }

    pub const fn check_step_syntax(self, syntax: StepSyntax) -> Result<(), ActionlintError> {
        let (supported, name) = match syntax {
            StepSyntax::JobMatrix => (self.job_matrix, "job_matrix"),
            StepSyntax::NativeParallelism => (self.native_step_parallelism, "native_parallelism"),
            StepSyntax::CacheMode => (self.cache_mode, "cache_mode"),
        };
        if supported {
            Ok(())
        } else {
            Err(ActionlintError::UnsupportedSyntax { syntax: name })
        }
    }
}

impl Default for JactionlintCapabilities {
    fn default() -> Self {
        Self::for_pinned()
    }
}
