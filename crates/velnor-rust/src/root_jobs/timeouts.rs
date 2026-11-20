// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Job, LaneJob};

use super::RootJobs;

impl RootJobs<'_> {
    /// The time limit of the root's jobs: its `[root_timeouts]` entry, else the repository value.
    /// A grouped unit runs all its roots in one job, so it takes the largest entry of them.
    pub(super) fn timeout_minutes(&self) -> u16 {
        self.unit
            .roots
            .iter()
            .filter_map(|root| self.facts.config.root_timeouts.get(&root.manifest.parent()))
            .copied()
            .max()
            .unwrap_or(self.facts.config.timeout_minutes)
    }

    /// An inline `[[test_lane]]` runs in its root's test job, so that job takes the larger of its
    /// own limit and the lane's. A parallel lane has a job of its own and does not come here.
    pub(super) fn inline_lanes_timeout(&self, mut job: Job) -> Job {
        let inline = self
            .facts
            .config
            .test_lanes
            .iter()
            .filter(|lane| {
                lane.job == LaneJob::Inline
                    && self
                        .unit
                        .roots
                        .iter()
                        .any(|root| root.manifest.parent() == lane.root)
            })
            .filter_map(|lane| lane.timeout_minutes)
            .max();
        if let Some(minutes) = inline {
            job.timeout_minutes = job.timeout_minutes.max(minutes);
        }
        job
    }
}
