// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use velnor_runner_github::{InnerKind, Poll};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PollObservation {
    Complete {
        available_events: u32,
        statistics: Option<PollStatistics>,
    },
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PollStatistics {
    pub(crate) available_jobs: i64,
    pub(crate) acquired_jobs: i64,
    pub(crate) assigned_jobs: i64,
    pub(crate) running_jobs: i64,
    pub(crate) registered_runners: i64,
    pub(crate) busy_runners: i64,
    pub(crate) idle_runners: i64,
}

pub(crate) fn poll_observation(polled: &Poll) -> PollObservation {
    match polled {
        Poll::Empty => PollObservation::Complete {
            available_events: 0,
            statistics: None,
        },
        Poll::Batch(batch) => batch_observation(batch),
        Poll::Quarantined(_) => PollObservation::Unavailable,
    }
}

fn batch_observation(batch: &velnor_runner_github::ParsedBatch) -> PollObservation {
    let available_events = batch
        .jobs
        .iter()
        .filter(|job| job.kind == InnerKind::Available)
        .count();
    let Ok(available_events) = u32::try_from(available_events) else {
        return PollObservation::Unavailable;
    };
    PollObservation::Complete {
        available_events,
        statistics: batch.statistics.as_ref().map(|stats| PollStatistics {
            available_jobs: stats.total_available_jobs,
            acquired_jobs: stats.total_acquired_jobs,
            assigned_jobs: stats.total_assigned_jobs,
            running_jobs: stats.total_running_jobs,
            registered_runners: stats.total_registered_runners,
            busy_runners: stats.total_busy_runners,
            idle_runners: stats.total_idle_runners,
        }),
    }
}

pub(crate) const RETRY: Duration = Duration::from_secs(5);

pub(crate) fn assigned_total(polled: &Poll) -> Option<i64> {
    match polled {
        Poll::Batch(batch) => batch
            .statistics
            .as_ref()
            .map(velnor_runner_github::Statistics::assigned_population),
        Poll::Empty | Poll::Quarantined(_) => None,
    }
}

pub(crate) fn poll_cursor(cursor: i64, retry_at: Option<Instant>, now: Instant) -> i64 {
    match retry_at {
        Some(at) if cursor > 0 && now >= at => 0,
        _ => cursor,
    }
}

pub(super) fn response_message_id(polled: &Poll) -> Option<i64> {
    match polled {
        Poll::Batch(batch) => Some(batch.message_id),
        Poll::Quarantined(batch) => Some(batch.message_id),
        Poll::Empty => None,
    }
}

pub(crate) fn steady_cursor(cursor: i64, message_id: i64) -> Option<i64> {
    if message_id > 0 {
        Some(cursor.max(message_id))
    } else {
        None
    }
}
