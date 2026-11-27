// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const FAILURE_LIMIT: u32 = 3;

pub(super) enum RunnerHealth {
    Working { failures: u32 },
    Failing { binary: String, checking: bool },
}

impl RunnerHealth {
    pub(super) const fn new() -> Self {
        Self::Working { failures: 0 }
    }

    pub(super) const fn is_failing(&self) -> bool {
        matches!(self, Self::Failing { .. })
    }

    pub(super) fn exited(&mut self, success: bool, binary: &str) -> Option<u32> {
        let Self::Working { failures } = self else {
            return None;
        };
        if success {
            *failures = 0;
            return None;
        }
        *failures = failures.saturating_add(1);
        let count = *failures;
        if count >= FAILURE_LIMIT {
            *self = Self::Failing {
                binary: binary.to_owned(),
                checking: false,
            };
            return Some(count);
        }
        None
    }

    pub(super) fn begin_check(&mut self) -> bool {
        match self {
            Self::Failing { checking, .. } if !*checking => {
                *checking = true;
                true
            }
            _ => false,
        }
    }

    pub(super) fn checked(&mut self, digest: Option<String>) -> bool {
        let Self::Failing { binary, checking } = self else {
            return false;
        };
        *checking = false;
        if digest.is_some_and(|current| current != *binary) {
            *self = Self::new();
            return true;
        }
        false
    }
}
