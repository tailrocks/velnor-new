use serde::Deserialize;

/// `statistics` object from the pinned Go struct.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Statistics {
    /// Jobs waiting.
    pub total_available_jobs: i64,
    /// Jobs acquired.
    pub total_acquired_jobs: i64,
    /// Population authority. Not the length of this batch.
    pub total_assigned_jobs: i64,
    /// Jobs running.
    pub total_running_jobs: i64,
    /// Registered runners.
    pub total_registered_runners: i64,
    /// Busy runners.
    pub total_busy_runners: i64,
    /// Idle runners.
    pub total_idle_runners: i64,
}

impl Statistics {
    /// Assigned population. Do not add the batch length to this.
    #[must_use]
    pub const fn assigned_population(&self) -> i64 {
        self.total_assigned_jobs
    }
}

/// Wire form of service statistics. Wire responses can omit counters,
/// but only the assigned population is required to make a snapshot useful.
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
