use std::sync::atomic::{AtomicUsize, Ordering};

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};

use super::*;
use support::{MissingSourceFixture, poll_fixture};

#[path = "tests/support.rs"]
mod support;

#[tokio::test]
async fn missing_source_blocks_live_admission_before_docker_or_reservation() -> Result<(), String> {
    let fixture = MissingSourceFixture::new().await?;
    let mut link = crate::listen::inert_link_for_offline_test();
    let polled = crate::launch_harness::available(&[91]);
    let Err(error) = Box::pin(fixture.admit_missing_source(&mut link, &polled)).await else {
        return Err("The admission code accepted a request without source data.".to_owned());
    };

    assert!(
        matches!(
            error,
            crate::scale_set::EnsureError::SourceBlocked {
                reason: "source_metadata_missing"
            }
        ),
        "Admission returned an unexpected error. Error: {error:?}"
    );
    fixture.assert_no_docker_request()?;
    assert!(fixture.reservations_empty().await?);
    fixture.cleanup()?;
    Ok(())
}

#[tokio::test]
async fn population_without_available_offer_never_invokes_admission() -> Result<(), String> {
    let mut fixture = poll_fixture().await?;
    let mut link = crate::listen::inert_link_for_offline_test();
    let admission_calls = AtomicUsize::new(0);
    let outcome = crate::launch::test_positive_poll_once_with_response(
        crate::launch::PositivePollContext {
            link: &mut link,
            set_id: 10_101,
            session: &mut fixture.session,
            scoped: &fixture.scope,
            session_intent_id: fixture.session_intent_id,
            allocation: &fixture.allocation,
            cursor: 0,
        },
        crate::launch::TestPollResponse {
            polled: Poll::Batch(ParsedBatch {
                message_id: 91,
                raw_body: String::new(),
                statistics: Some(velnor_runner_github::Statistics {
                    total_available_jobs: 0,
                    total_acquired_jobs: 0,
                    total_assigned_jobs: 1,
                    total_running_jobs: 0,
                    total_registered_runners: 0,
                    total_busy_runners: 0,
                    total_idle_runners: 0,
                }),
                jobs: vec![InnerJob {
                    kind: InnerKind::Assigned,
                    request_id: Some(92),
                    job_id: None,
                    labels: Vec::new(),
                    runner_id: None,
                    runner_name: None,
                    result: None,
                    fields: Vec::new(),
                    source: velnor_runner_github::SourceMetadata::default(),
                }],
            }),
            path: "https://queue.example.invalid/messages".to_owned(),
            queue: Some("queue-token".to_owned()),
        },
        async |_link, _session, _path, _queue, _polled| Ok(()),
        async |_link, _polled, _origin, _path, _queue, _admin| {
            admission_calls.fetch_add(1, Ordering::Relaxed);
            Err(crate::scale_set::EnsureError::AdmissionClosed)
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(outcome.assigned_total, Some(1));
    assert!(outcome.admission.is_none());
    assert_eq!(admission_calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        fixture
            .scope
            .physical_runner_reservations()
            .await
            .map_err(|error| error.to_string())?,
        Vec::<crate::journal::PhysicalRunnerReservation>::new()
    );
    drop(fixture);
    Ok(())
}
