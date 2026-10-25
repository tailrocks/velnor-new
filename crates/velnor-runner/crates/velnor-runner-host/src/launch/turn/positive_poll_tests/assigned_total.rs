use velnor_runner_github::{ParsedBatch, Poll, Statistics};

use super::{Acknowledgement, IntakeDecision, Settlement, make_outcome};

#[test]
fn positive_outcome_exposes_native_assigned_total_and_preserves_absence() {
    let reported = Poll::Batch(ParsedBatch {
        message_id: 19,
        raw_body: String::new(),
        statistics: Some(Statistics {
            total_available_jobs: 4,
            total_acquired_jobs: 2,
            total_assigned_jobs: 6,
            total_running_jobs: 1,
            total_registered_runners: 3,
            total_busy_runners: 1,
            total_idle_runners: 2,
        }),
        jobs: Vec::new(),
    });
    let decision = IntakeDecision {
        completion_only: false,
        wake_cleanup: false,
        session_delete_candidate: false,
        started_request_ids: Vec::new(),
    };
    let outcome = make_outcome::<()>(
        decision.clone(),
        None,
        Acknowledgement::NotRequired,
        Settlement::Settled,
        &reported,
    );
    assert_eq!(outcome.assigned_total, Some(6));

    let missing_batch = Poll::Batch(ParsedBatch {
        message_id: 20,
        raw_body: String::new(),
        statistics: None,
        jobs: Vec::new(),
    });
    let missing = make_outcome::<()>(
        decision.clone(),
        None,
        Acknowledgement::NotRequired,
        Settlement::Settled,
        &missing_batch,
    );
    assert_eq!(missing.assigned_total, None);

    let empty = make_outcome::<()>(
        decision,
        None,
        Acknowledgement::NotRequired,
        Settlement::Settled,
        &Poll::Empty,
    );
    assert_eq!(empty.assigned_total, None);
}
