// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

const DEFAULT_STORE_BUDGET_GIB: u64 = 100;
pub(crate) const DEFAULT_JOB_LIMIT_GIB: u64 = 64;
const DEFAULT_JOB_SLACK_GIB: u64 = 8;
pub(crate) const DEFAULT_ONE_JOB_CAP_GIB: u64 =
    DEFAULT_STORE_BUDGET_GIB + DEFAULT_JOB_LIMIT_GIB + DEFAULT_JOB_SLACK_GIB;
const MAX_CAP_GIB: u64 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct MachineDisk {
    pub store_budget_gib: u64,
    pub job_limit_gib: u64,
    pub job_slack_gib: u64,
}

impl MachineDisk {
    pub(crate) const DEFAULT: Self = Self {
        store_budget_gib: DEFAULT_STORE_BUDGET_GIB,
        job_limit_gib: DEFAULT_JOB_LIMIT_GIB,
        job_slack_gib: DEFAULT_JOB_SLACK_GIB,
    };

    pub(crate) fn is_default(&self) -> bool {
        *self == Self::DEFAULT
    }

    #[must_use]
    pub const fn cap_gib(self, max_jobs: u64) -> Option<u64> {
        let Some(per_job) = self.job_limit_gib.checked_add(self.job_slack_gib) else {
            return None;
        };
        let Some(jobs) = per_job.checked_mul(max_jobs) else {
            return None;
        };
        match self.store_budget_gib.checked_add(jobs) {
            Some(cap) if self.job_limit_gib > 0 && cap <= MAX_CAP_GIB => Some(cap),
            _ => None,
        }
    }
}

impl Default for MachineDisk {
    fn default() -> Self {
        Self::DEFAULT
    }
}
