//! Queue-offer classification and malformed-batch replay tests.

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll, Statistics};

use crate::launch::harness::{
    Mode, Script, absent, assigned_wait, available, ctx, open, started_progress,
};
use crate::launch::{Idle, drive_offer_tracked, idle};
use velnor_runner_host::{EnsureError, Started};

#[test]
fn statistics_advance_and_offers_stay() {
    let stats = Poll::Batch(ParsedBatch {
        message_id: 2,
        statistics: None,
        jobs: Vec::new(),
    });
    assert_eq!(idle(&stats), Idle::Ack);
    assert_eq!(idle(&available(&[3])), Idle::Launch);
    assert_eq!(idle(&Poll::Empty), Idle::Empty);
    assert_eq!(idle(&available(&[3, 4])), Idle::Blocked);
    assert_eq!(idle(&assigned_wait(7, 1)), Idle::Scale);
    assert_eq!(idle(&assigned_wait(7, 0)), Idle::Ack);
    assert_eq!(idle(&assigned_wait(8, -1)), Idle::Blocked);
    assert_eq!(idle(&started_progress(11, 5)), Idle::Scale);

    assert_eq!(idle(&no_stats(vec![job(InnerKind::Started)])), Idle::Ack);
    assert_eq!(idle(&no_stats(vec![job(InnerKind::Completed)])), Idle::Ack);
    assert_eq!(
        idle(&no_stats(vec![
            job(InnerKind::Started),
            job(InnerKind::Completed),
        ])),
        Idle::Ack
    );
    assert_eq!(
        idle(&no_stats(vec![job(InnerKind::Assigned)])),
        Idle::Blocked
    );
    assert_eq!(
        idle(&no_stats(vec![job(InnerKind::Available)])),
        Idle::Blocked
    );
    assert_eq!(
        idle(&no_stats(vec![
            job(InnerKind::Started),
            job(InnerKind::Available),
        ])),
        Idle::Blocked
    );
    let mut available_with_id = job(InnerKind::Available);
    available_with_id.request_id = Some(9);
    assert_eq!(
        idle(&no_stats(vec![job(InnerKind::Started), available_with_id])),
        Idle::Launch
    );
    assert_eq!(
        idle(&no_stats(vec![job(InnerKind::Unsupported(
            "FutureKind".to_owned(),
        ))])),
        Idle::Blocked
    );
    assert_eq!(
        idle(&batch(
            Some(Statistics {
                total_available_jobs: 0,
                total_acquired_jobs: 0,
                total_assigned_jobs: -1,
                total_running_jobs: 0,
                total_registered_runners: 0,
                total_busy_runners: 0,
                total_idle_runners: 0,
            }),
            vec![job(InnerKind::Started)],
        )),
        Idle::Blocked
    );
    let synthetic = ParsedBatch {
        message_id: -1,
        statistics: None,
        jobs: Vec::new(),
    };
    assert_eq!(idle(&Poll::Batch(synthetic)), Idle::Blocked);
}

#[tokio::test]
async fn malformed_available_in_mixed_wire_batch_is_redelivered_without_effects()
-> Result<(), String> {
    let (scratch, journal) = open("malformed-available-redelivery").await?;
    let body = serde_json::json!([
        {"messageType": "JobAvailable", "runnerRequestId": 9, "jobId": "job-9"},
        {"messageType": "JobAvailable", "jobId": "job-missing-request"},
        {"messageType": "JobCompleted", "jobId": "job-8", "runnerId": 2, "runnerName": "runner-2"}
    ])
    .to_string();
    let wire = serde_json::json!({
        "messageId": 77,
        "messageType": "RunnerScaleSetJobMessages",
        "body": body,
        "statistics": {
            "totalAvailableJobs": 1,
            "totalAcquiredJobs": 0,
            "totalAssignedJobs": 1,
            "totalRunningJobs": 0,
            "totalRegisteredRunners": 1,
            "totalBusyRunners": 0,
            "totalIdleRunners": 1
        }
    })
    .to_string();
    let poll = velnor_runner_github::parse_poll(200, &wire)
        .map_err(|_| "mixed Scale Set wire message did not parse".to_owned())?;
    assert_eq!(idle(&poll), Idle::Blocked);

    let mut script = Script {
        calls: Vec::new(),
        mode: Mode::Ok,
    };
    for _ in 0..2 {
        let result = drive_offer_tracked(
            &mut script,
            &ctx(),
            &poll,
            &journal,
            |_volume, _jit, _bind| async {
                Ok(Started {
                    dind_id: "dind-never-started".to_owned(),
                    runner_id: "runner-never-started".to_owned(),
                })
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(EnsureError::Unexpected {
                status: 0,
                step: "queue message"
            })
        ));
        assert_eq!(script.calls, Vec::<&'static str>::new());
        assert_eq!(
            journal.rows().await.map_err(|error| error.to_string())?,
            Vec::<velnor_runner_journal::reconcile::IntentRow>::new()
        );
    }
    absent(&scratch.file())
}

fn no_stats(jobs: Vec<InnerJob>) -> Poll {
    batch(None, jobs)
}

fn batch(statistics: Option<Statistics>, jobs: Vec<InnerJob>) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 77,
        statistics,
        jobs,
    })
}

fn job(kind: InnerKind) -> InnerJob {
    InnerJob {
        kind,
        request_id: None,
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
