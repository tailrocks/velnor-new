// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const FAILURE_LIMIT: u32 = 3;

pub(super) enum Change {
    Recorded,
    Replaced,
}

pub(super) enum RunnerHealth {
    Working {
        failures: u32,
    },
    Failing {
        baseline: Option<String>,
        checking: bool,
    },
}

impl RunnerHealth {
    pub(super) const fn new() -> Self {
        Self::Working { failures: 0 }
    }

    pub(super) const fn is_failing(&self) -> bool {
        matches!(self, Self::Failing { .. })
    }

    pub(super) fn exited(&mut self, success: bool) -> Option<u32> {
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
                baseline: None,
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

    pub(super) fn checked(&mut self, digest: Option<String>) -> Option<Change> {
        let Self::Failing { baseline, checking } = self else {
            return None;
        };
        *checking = false;
        let digest = digest?;
        match baseline {
            None => {
                *baseline = Some(digest);
                Some(Change::Recorded)
            }
            Some(known) if *known != digest => {
                *self = Self::new();
                Some(Change::Replaced)
            }
            Some(_) => None,
        }
    }
}
