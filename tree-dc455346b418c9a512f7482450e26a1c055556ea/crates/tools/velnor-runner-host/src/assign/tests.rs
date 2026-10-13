//! Poll offers. No transport.

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};

use crate::{Offer, offer};

fn batch(jobs: Vec<InnerJob>) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 4,
        statistics: None,
        jobs,
    })
}

fn job(kind: InnerKind, request_id: Option<i64>) -> InnerJob {
    InnerJob {
        kind,
        request_id,
        job_id: None,
        workflow_run_id: None,
        owner_name: None,
        repository_name: None,
        event_name: None,
        labels: Vec::new(),
        runner_id: None,
        runner_name: None,
        result: None,
        fields: Vec::new(),
    }
}

#[test]
fn empty_poll_waits() {
    assert_eq!(offer(&Poll::Empty), Offer::Wait);
}

#[test]
fn available_ids_are_the_only_acquire() {
    let poll = batch(vec![
        job(InnerKind::Available, Some(9)),
        job(InnerKind::Available, None),
        job(InnerKind::Completed, Some(3)),
        job(InnerKind::Assigned, Some(8)),
    ]);
    assert_eq!(
        offer(&poll),
        Offer::Acquire {
            message_id: 4,
            ids: vec![9],
        }
    );
}

#[test]
fn a_batch_without_an_available_id_waits() {
    let poll = batch(vec![job(InnerKind::Started, Some(1))]);
    assert_eq!(offer(&poll), Offer::Wait);
}
