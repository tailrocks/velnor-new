// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Statistics {
    pub total_available_jobs: i64,

    pub total_acquired_jobs: i64,

    pub total_assigned_jobs: i64,

    pub total_running_jobs: i64,

    pub total_registered_runners: i64,

    pub total_busy_runners: i64,

    pub total_idle_runners: i64,
}

impl Statistics {
    #[must_use]
    pub const fn assigned_population(&self) -> i64 {
        self.total_assigned_jobs
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct StatisticsWire {
    #[serde(default, rename = "totalAvailableJobs")]
    available_jobs: i64,
    #[serde(default, rename = "totalAcquiredJobs")]
    acquired_jobs: i64,
    #[serde(default, rename = "totalAssignedJobs")]
    assigned_jobs: Option<i64>,
    #[serde(default, rename = "totalRunningJobs")]
    running_jobs: i64,
    #[serde(default, rename = "totalRegisteredRunners")]
    registered_runners: i64,
    #[serde(default, rename = "totalBusyRunners")]
    busy_runners: i64,
    #[serde(default, rename = "totalIdleRunners")]
    idle_runners: i64,
}

pub(super) fn to_statistics(wire: &StatisticsWire) -> Option<Statistics> {
    Some(Statistics {
        total_available_jobs: wire.available_jobs,
        total_acquired_jobs: wire.acquired_jobs,
        total_assigned_jobs: wire.assigned_jobs?,
        total_running_jobs: wire.running_jobs,
        total_registered_runners: wire.registered_runners,
        total_busy_runners: wire.busy_runners,
        total_idle_runners: wire.idle_runners,
    })
}
