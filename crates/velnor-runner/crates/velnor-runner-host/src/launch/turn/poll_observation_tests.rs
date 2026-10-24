use crate::launch::steady::{PollObservation, PollStatistics, poll_observation};
use velnor_runner_github::{
    InnerJob, InnerKind, ParsedBatch, Poll, QuarantinedBatch, SourceMetadata, Statistics,
};

#[test]
fn empty_response_is_zero_observed_events_with_absent_statistics() {
    assert_eq!(
        poll_observation(&Poll::Empty),
        PollObservation::Complete {
            available_events: 0,
            statistics: None,
        }
    );
}

#[test]
fn batch_projection_counts_available_events_and_copies_all_counters() {
    let polled = Poll::Batch(ParsedBatch {
        message_id: 47,
        raw_body: "private-body-must-not-escape".to_owned(),
        statistics: Some(Statistics {
            total_available_jobs: 11,
            total_acquired_jobs: 12,
            total_assigned_jobs: 13,
            total_running_jobs: 14,
            total_registered_runners: 15,
            total_busy_runners: 16,
            total_idle_runners: 17,
        }),
        jobs: vec![job(InnerKind::Available), job(InnerKind::Started)],
    });
    let observation = poll_observation(&polled);
    assert_eq!(
        observation,
        PollObservation::Complete {
            available_events: 1,
            statistics: Some(PollStatistics {
                available_jobs: 11,
                acquired_jobs: 12,
                assigned_jobs: 13,
                running_jobs: 14,
                registered_runners: 15,
                busy_runners: 16,
                idle_runners: 17,
            }),
        }
    );
    let encoded = serde_json::to_string(&observation).expect("observation serializes");
    assert!(!encoded.contains("private-body"));
    assert!(!encoded.contains("47"));
    assert!(!encoded.contains("opaque-job-id"));
    assert!(!encoded.contains("100"));
}

#[test]
fn quarantined_response_is_unavailable_without_body_projection() {
    let observation = poll_observation(&Poll::Quarantined(QuarantinedBatch {
        message_id: 51,
        raw_body: "private-quarantine-body".to_owned(),
    }));
    assert_eq!(observation, PollObservation::Unavailable);
    let encoded = serde_json::to_string(&observation).expect("observation serializes");
    assert!(!encoded.contains("private-quarantine-body"));
    assert!(!encoded.contains("51"));
}

fn job(kind: InnerKind) -> InnerJob {
    InnerJob {
        kind,
        request_id: Some(100),
        runner_id: None,
        runner_name: None,
        result: None,
        job_id: Some("opaque-job-id".to_owned()),
        labels: Vec::new(),
        fields: Vec::new(),
        source: SourceMetadata::default(),
    }
}
