//! What one poll allows. No network and no acknowledgement.

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};

/// One poll outcome the controller can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// Nothing to acquire. Do not delete an unseen offer.
    Wait,
    /// Acquire these ids before acknowledging `message_id`.
    Acquire {
        /// Poll message id.
        message_id: i64,
        /// `runnerRequestId` values from `JobAvailable` only.
        ids: Vec<i64>,
    },
}

/// `JobAvailable` ids only. Other kinds do not free or acquire a slot.
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

/// True when every job is a start or completion notice. An empty batch is not progress.
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

/// Every `JobAssigned` shares `runnerRequestId` or `jobId` with one `JobStarted`.
///
/// A batch can carry more than one job. An assigned id with no started pair
/// stays a mint. Do not delete that message.
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

#[cfg(test)]
mod tests {
    use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch};

    use super::*;

    fn job(kind: InnerKind, request_id: Option<i64>, job_id: Option<&str>) -> InnerJob {
        InnerJob {
            kind,
            request_id,
            job_id: job_id.map(str::to_owned),
            labels: Vec::new(),
            runner_id: None,
            runner_name: None,
            result: None,
            fields: Vec::new(),
        }
    }

    fn batch(jobs: Vec<InnerJob>) -> ParsedBatch {
        ParsedBatch {
            message_id: 7,
            raw_body: String::new(),
            statistics: None,
            jobs,
        }
    }

    #[test]
    fn replay_requires_each_assigned_id_to_have_started() {
        assert!(started_replay(&batch(vec![
            job(InnerKind::Assigned, Some(4), None),
            job(InnerKind::Started, Some(4), None),
        ])));
        assert!(!started_replay(&batch(vec![
            job(InnerKind::Assigned, Some(4), None),
            job(InnerKind::Started, Some(9), None),
        ])));
        assert!(started_replay(&batch(vec![
            job(InnerKind::Assigned, None, Some("15")),
            job(InnerKind::Started, None, Some("15")),
        ])));
        assert!(!started_replay(&batch(vec![
            job(InnerKind::Assigned, Some(4), None),
            job(InnerKind::Assigned, Some(5), None),
            job(InnerKind::Started, Some(4), None),
        ])));
        assert!(!started_replay(&batch(vec![
            job(InnerKind::Assigned, None, None),
            job(InnerKind::Started, Some(4), None),
        ])));
        assert!(!started_replay(&batch(vec![
            job(InnerKind::Assigned, Some(0), None),
            job(InnerKind::Started, Some(0), None),
        ])));
    }

    #[test]
    fn numeric_job_id_pairs_when_request_id_is_zero() {
        let same = r#"{"messageId":9,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAssigned\",\"runnerRequestId\":0,\"jobId\":111618125414},{\"messageType\":\"JobStarted\",\"runnerRequestId\":0,\"jobId\":111618125414}]"}"#;
        let velnor_runner_github::Poll::Batch(batch) =
            velnor_runner_github::parse_poll(200, same).expect("poll")
        else {
            panic!("batch");
        };
        assert!(started_replay(&batch));
        let split = r#"{"messageId":9,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAssigned\",\"runnerRequestId\":0,\"jobId\":1},{\"messageType\":\"JobStarted\",\"runnerRequestId\":0,\"jobId\":2}]"}"#;
        let velnor_runner_github::Poll::Batch(batch) =
            velnor_runner_github::parse_poll(200, split).expect("poll")
        else {
            panic!("batch");
        };
        assert!(!started_replay(&batch));
    }
}
