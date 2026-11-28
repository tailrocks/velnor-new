// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::JobId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReuseStep {
    Record,
    Upload,
    Decide { required: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reuse {
    pub id: String,
    pub plan: JobId,
}

impl Reuse {
    #[must_use]
    pub fn job(job: &JobId, plan: &JobId) -> Self {
        Self {
            id: job.to_string(),
            plan: plan.clone(),
        }
    }

    #[must_use]
    pub fn step(job: &JobId, kind: &str, position: usize, plan: &JobId) -> Self {
        Self {
            id: format!("{job}.{kind}-{position}"),
            plan: plan.clone(),
        }
    }
}
