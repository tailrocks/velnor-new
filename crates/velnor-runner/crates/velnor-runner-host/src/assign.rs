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

fn available_ids(batch: &ParsedBatch) -> Vec<i64> {
    batch
        .jobs
        .iter()
        .filter(|job| matches!(job.kind, InnerKind::Available | InnerKind::Assigned))
        .filter_map(|job| job.request_id)
        .collect()
}
