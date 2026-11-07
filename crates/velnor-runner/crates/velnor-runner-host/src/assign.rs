// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{InnerKind, ParsedBatch, Poll};

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

pub(crate) fn is_active_offer_job(
    batch: &ParsedBatch,
    job: &velnor_runner_github::InnerJob,
) -> bool {
    if job.request_id.is_none()
        || batch
            .jobs
            .iter()
            .any(|other| other.kind == InnerKind::Completed && other.request_id == job.request_id)
    {
        return false;
    }
    match job.kind {
        InnerKind::Available => !is_superseded_by_assigned(batch, job),
        InnerKind::Assigned => true,
        InnerKind::Started | InnerKind::Completed | InnerKind::Unsupported(_) => false,
    }
}

fn is_superseded_by_assigned(batch: &ParsedBatch, job: &velnor_runner_github::InnerJob) -> bool {
    let available_count = batch
        .jobs
        .iter()
        .filter(|other| other.kind == InnerKind::Available && other.request_id == job.request_id)
        .count();
    let matching_assigned_count = batch
        .jobs
        .iter()
        .filter(|other| {
            other.kind == InnerKind::Assigned
                && other.request_id == job.request_id
                && other.source == job.source
                && other.job_id == job.job_id
        })
        .count();
    available_count == 1 && matching_assigned_count == 1
}

fn available_ids(batch: &ParsedBatch) -> Vec<i64> {
    batch
        .jobs
        .iter()
        .filter(|job| is_active_offer_job(batch, job))
        .filter_map(|job| job.request_id)
        .collect()
}
