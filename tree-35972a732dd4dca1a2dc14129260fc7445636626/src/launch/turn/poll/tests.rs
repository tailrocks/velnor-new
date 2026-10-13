//! Scripted waves through the listener's production admission and dispatch seam.

use velnor_runner_github::{
    ImmutableJobContext, InnerJob, InnerKind, ParsedBatch, Poll, Statistics,
};

use crate::error::PreparationCause;
use crate::journal::{Journal, LaunchReservation};
use crate::launch::capacity::install_job_capacity;
use crate::launch::turn::poll::{DispatchFuture, PollDispatcher, apply};
use crate::launch::{Drive, Lane, drive_offer_reserved};
use crate::launch_harness::{Mode, Scratch, Script, assigned_wait, available};
use crate::worker::{PreparedDind, Started};
use crate::{EnsureError, HostError};

const SET_ID: i64 = 1;

struct Waves {
    script: Script,
    fail_next_preparation: bool,
    start_count: usize,
}

impl Waves {
    fn new() -> Self {
        Self {
            script: Script {
                calls: Vec::new(),
                mode: Mode::Ok,
            },
            fail_next_preparation: false,
            start_count: 0,
        }
    }
}

impl PollDispatcher for Waves {
    fn ack(
        &mut self,
        _path: String,
        _queue: Option<String>,
        polled: &Poll,
    ) -> Result<(), EnsureError> {
        let Poll::Batch(batch) = polled else {
            return Ok(());
        };
        let context = drive_context();
        crate::launch::steps_ack::acknowledge(&mut self.script, &context, batch)
    }

    fn start<'a>(
        &'a mut self,
        journal: &'a Journal,
        path: String,
        _queue: Option<String>,
        polled: &'a Poll,
        reservation: Option<LaunchReservation>,
    ) -> DispatchFuture<'a> {
        let script = &mut self.script;
        let fail_preparation = std::mem::take(&mut self.fail_next_preparation);
        self.start_count = self.start_count.saturating_add(1);
        let start_number = self.start_count;
        Box::pin(async move {
            let mut context = drive_context();
            context.queue_path = path;
            drive_offer_reserved(
                script,
                &context,
                polled,
                journal,
                reservation,
                move |identity| async move {
                    if fail_preparation {
                        return Err(HostError::PreparationFailedClean(
                            PreparationCause::DindReadiness,
                        ));
                    }
                    let dind_id = format!("{start_number:064x}");
                    PreparedDind::from_journal(&identity, &dind_id)
                },
                move |_identity, prepared, _jit| async move {
                    Ok(Started {
                        dind_id: prepared.dind_id().to_owned(),
                        runner_id: format!("runner-{start_number}"),
                    })
                },
            )
            .await
        })
    }
}

fn drive_context() -> Drive {
    Drive {
        set_id: SET_ID,
        queue_path: "queues/messages".to_owned(),
        queue_token: "queue-token".to_owned(),
        admin_token: "admin-token".to_owned(),
    }
}

async fn open(scratch: &Scratch) -> Result<Journal, String> {
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("docker-engine-test")
        .await
        .map_err(|error| error.to_string())?;
    Ok(journal)
}

async fn apply_poll(
    journal: &Journal,
    dispatcher: &mut Waves,
    workers: &mut Vec<Started>,
    polled: &Poll,
) -> Result<bool, EnsureError> {
    apply(
        journal,
        SET_ID,
        1,
        1,
        workers,
        0,
        polled,
        "queues/messages".to_owned(),
        None,
        dispatcher,
    )
    .await
}

async fn mark_completed_and_clean(
    journal: &Journal,
    dispatcher: &mut Waves,
    workers: &mut Vec<Started>,
    request_id: i64,
) -> Result<(), String> {
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    let row = rows
        .iter()
        .find(|row| row.request_id == Some(request_id))
        .ok_or_else(|| "completed assignment was not journaled".to_owned())?;
    let launch_id = row
        .launch_id
        .as_deref()
        .ok_or_else(|| "completed assignment has no launch id".to_owned())?;
    let runner_name = format!("v{launch_id}");
    let completed = Poll::Batch(ParsedBatch {
        message_id: 200 + request_id,
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Completed,
            request_id: Some(request_id),
            context: empty_context(),
            runner_id: Some(71),
            runner_name: Some(runner_name),
            result: None,
            fields: Vec::new(),
        }],
    });
    crate::launch::completion::record_completion_events(journal, SET_ID, &completed)
        .await
        .map_err(|error| error.to_string())?;
    apply_poll(journal, dispatcher, workers, &completed)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .mark_completion_worker_cleanup_proven(row.id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_cleanup(row.id)
        .await
        .map_err(|error| error.to_string())
}

fn empty_context() -> ImmutableJobContext {
    ImmutableJobContext {
        repository_name: None,
        owner_name: None,
        job_id: None,
        job_workflow_ref: None,
        job_display_name: None,
        workflow_run_id: None,
        event_name: None,
        request_labels: Vec::new(),
    }
}

fn assigned(message_id: i64) -> Poll {
    let mut poll = assigned_wait(message_id, 1);
    if let Poll::Batch(batch) = &mut poll {
        batch.statistics = Some(Statistics {
            total_available_jobs: 0,
            total_acquired_jobs: 0,
            total_assigned_jobs: 1,
            total_running_jobs: 0,
            total_registered_runners: 0,
            total_busy_runners: 0,
            total_idle_runners: 0,
        });
    }
    poll
}

#[tokio::test]
async fn production_dispatch_refills_after_completion_restart_and_clean_failure()
-> Result<(), String> {
    let _capacity = install_job_capacity(1);
    let scratch = Scratch::new("poll-three-waves").map_err(|error| error.to_string())?;
    let journal = open(&scratch).await?;
    let mut workers = Vec::new();
    let mut dispatcher = Waves::new();

    assert!(apply_poll(&journal, &mut dispatcher, &mut workers, &available(&[42])).await?);
    assert_eq!(workers.len(), 1);
    mark_completed_and_clean(&journal, &mut dispatcher, &mut workers, 42).await?;

    drop(journal);
    let journal = open(&scratch).await?;
    workers.clear();
    dispatcher.fail_next_preparation = true;
    assert_eq!(
        apply_poll(&journal, &mut dispatcher, &mut workers, &assigned(101)).await,
        Err(EnsureError::Unexpected {
            status: 0,
            step: "dind-ready",
        })
    );
    assert_eq!(journal.occupied_launches().await, Ok(0));
    let calls_after_failure = dispatcher.script.calls.clone();
    assert!(!apply_poll(&journal, &mut dispatcher, &mut workers, &assigned(101)).await?);
    assert_eq!(dispatcher.script.calls, calls_after_failure);

    drop(journal);
    let journal = open(&scratch).await?;
    workers.clear();
    assert!(apply_poll(&journal, &mut dispatcher, &mut workers, &available(&[43])).await?);
    assert_eq!(workers.len(), 1);
    assert_eq!(
        dispatcher.script.calls,
        ["acquire", "jit", "ack", "ack", "acquire", "jit", "ack"]
    );
    assert_eq!(dispatcher.start_count, 4);
    assert_eq!(journal.occupied_launches().await, Ok(1));
    Ok(())
}
