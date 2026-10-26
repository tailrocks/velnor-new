// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    Wait,

    Acquire { message_id: i64, ids: Vec<i64> },
}

#[must_use]
pub fn offer(poll: &Poll) -> Offer {
    match poll {
        Poll::Empty | Poll::Quarantined(_) => Offer::Wait,
        Poll::Batch(batch) => acquire_offer(batch),
    }
}

fn acquire_offer(batch: &ParsedBatch) -> Offer {
    let ids = available_ids(batch);
    if ids.is_empty() {
        Offer::Wait
    } else {
        Offer::Acquire {
            message_id: batch.message_id,
            ids,
        }
    }
}

#[must_use]
pub(crate) fn progress_only(batch: &ParsedBatch) -> bool {
    !batch.jobs.is_empty()
        && batch
            .jobs
            .iter()
            .all(|job| matches!(job.kind, InnerKind::Started | InnerKind::Completed))
}

fn available_ids(batch: &ParsedBatch) -> Vec<i64> {
    batch
        .jobs
        .iter()
        .filter(|job| matches!(job.kind, InnerKind::Available))
        .filter_map(|job| job.request_id)
        .collect()
}

#[must_use]
pub(crate) fn started_replay(batch: &ParsedBatch) -> bool {
    let assigned: Vec<&InnerJob> = batch
        .jobs
        .iter()
        .filter(|job| matches!(job.kind, InnerKind::Assigned))
        .collect();
    !assigned.is_empty()
        && assigned
            .iter()
            .all(|job| batch.jobs.iter().any(|other| paired(job, other)))
}

fn paired(assigned: &InnerJob, started: &InnerJob) -> bool {
    if !matches!(started.kind, InnerKind::Started) {
        return false;
    }
    let same_request = assigned
        .request_id
        .is_some_and(|id| id > 0 && started.request_id == Some(id));
    let same_job = assigned.job_id.is_some() && assigned.job_id == started.job_id;
    same_request || same_job
}
