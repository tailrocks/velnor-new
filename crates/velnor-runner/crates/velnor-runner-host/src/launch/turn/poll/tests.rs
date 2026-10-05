//! Capacity-one waves through the listener's production wait loop.

use std::collections::VecDeque;

use velnor_runner_github::{ImmutableJobContext, InnerJob, InnerKind, ParsedBatch, Poll};

use crate::error::PreparationCause;
use crate::journal::{Journal, LaunchReservation};
use crate::launch::capacity::install_job_capacity;
use crate::launch::completion::{self, BlockingRunnerApi, CompletionEngine};
use crate::launch::turn::poll::{
    DispatchFuture, LoopDriver, LoopFuture, PollDispatcher, apply, until_idle,
};
use crate::launch::{Drive, drive_offer_reserved};
use crate::launch_harness::{Mode, Scratch, assigned_wait, available};
use crate::worker::{PreparedDind, Started};
use crate::{EnsureError, HostError};

const SET_ID: i64 = 1;
const CAPACITY: u32 = 1;

struct Waves {
    script: crate::launch_harness::Script,
    fail_message: Option<i64>,
    start_count: usize,
}

impl Waves {
    fn new(fail_message: i64) -> Self {
        Self {
            script: crate::launch_harness::Script {
                calls: Vec::new(),
                mode: Mode::Ok,
            },
            fail_message: Some(fail_message),
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
        crate::launch::steps_ack::acknowledge(&mut self.script, &drive_context(), batch)
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
        let fail = self.fail_message == batch_message(polled);
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
                    if fail {
                        return Err(HostError::PreparationFailedClean(
                            PreparationCause::DindReadiness,
                        ));
                    }
                    let dind_id = format!("{:064x}", start_number + 10);
                    PreparedDind::from_journal(&identity, &dind_id)
                },
                move |_identity, prepared, _jit| async move {
                    let runner_id = format!("{start_number:064x}");
                    Ok(Started {
                        dind_id: prepared.dind_id().to_owned(),
                        runner_id,
                    })
                },
            )
            .await
        })
    }
}

struct ListenerSession {
    journal: Journal,
    polls: VecDeque<Poll>,
    dispatcher: Waves,
    running: u32,
    seen: usize,
    cleanups: Vec<tokio::task::JoinHandle<()>>,
}

impl LoopDriver for ListenerSession {
    fn drive<'a>(&'a mut self, workers: &'a mut Vec<Started>) -> LoopFuture<'a, bool> {
        Box::pin(async move {
            let mut polled = self.polls.pop_front().unwrap_or(Poll::Empty);
            self.seen = self.seen.saturating_add(1);
            if schedule_completion(&self.journal, &mut polled, &mut self.cleanups).await? {
                self.running = 0;
            }
            let before = workers.len();
            let stop = apply(
                &self.journal,
                SET_ID,
                CAPACITY,
                CAPACITY,
                workers,
                self.running,
                &polled,
                "queues/messages".to_owned(),
                None,
                &mut self.dispatcher,
            )
            .await?;
            if workers.len() > before {
                self.running = self.running.saturating_add(1);
            }
            Ok(stop)
        })
    }

    fn running<'a>(&'a mut self) -> LoopFuture<'a, u32> {
        Box::pin(async move { Ok(self.running) })
    }

    fn has_pending_cleanup<'a>(&'a mut self) -> LoopFuture<'a, bool> {
        Box::pin(async move {
            self.journal.completed_launches().await.map_or_else(
                |error| {
                    eprintln!("poll-test completed launches failed: {error:?}");
                    Err(EnsureError::Unexpected {
                        status: 0,
                        step: "test-journal",
                    })
                },
                |rows| Ok(!rows.is_empty()),
            )
        })
    }

    fn pause<'a>(&'a mut self, _duration: std::time::Duration) -> LoopFuture<'a, ()> {
        Box::pin(async move {
            for cleanup in self.cleanups.drain(..) {
                cleanup.await.map_err(|_| EnsureError::Unexpected {
                    status: 0,
                    step: "test-cleanup",
                })?;
            }
            tokio::task::yield_now().await;
            Ok(())
        })
    }
}

async fn schedule_completion(
    journal: &Journal,
    polled: &mut Poll,
    cleanups: &mut Vec<tokio::task::JoinHandle<()>>,
) -> Result<bool, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(false);
    };
    let Some(index) = batch
        .jobs
        .iter()
        .position(|job| job.kind == InnerKind::Completed)
    else {
        return Ok(false);
    };
    let request_id = batch.jobs[index]
        .request_id
        .ok_or_else(test_journal_error)?;
    let assignment_key = format!("{SET_ID}:{request_id}");
    let rows = journal.rows().await.map_err(|error| {
        eprintln!("poll-test journal rows failed: {error:?}");
        test_journal_error()
    })?;
    let row = rows
        .iter()
        .find(|row| row.assignment_key.as_deref() == Some(&assignment_key))
        .cloned()
        .ok_or_else(|| {
            let summary: Vec<_> = rows
                .iter()
                .map(|row| {
                    (
                        row.id,
                        row.assignment_key.as_deref(),
                        row.state,
                        row.cleanup_proven,
                    )
                })
                .collect();
            eprintln!("poll-test assignment row missing: wanted={assignment_key} rows={summary:?}");
            test_journal_error()
        })?;
    let identity = journal.launch_identity(row.id).await.map_err(|error| {
        eprintln!("poll-test launch identity failed: {error:?}");
        test_journal_error()
    })?;
    let name = format!("v{}", identity.launch_id());
    batch.jobs[index].runner_name = Some(name.clone());
    completion::record_completion_events(journal, SET_ID, polled).await?;
    let runner_id = row.docker_id.ok_or_else(test_journal_error)?;
    let dind_id = row.dind_id.ok_or_else(test_journal_error)?;
    let engine = CompletionEngine::with_stopped_pair(&identity, &runner_id, &dind_id)
        .map_err(|_| test_journal_error())?;
    let api = BlockingRunnerApi::released_for_scale_set(&name, 71, SET_ID);
    cleanups.extend(
        completion::schedule_completed_isolated(
            api,
            SET_ID,
            "admin-token",
            journal.clone(),
            engine,
        )
        .await?,
    );
    Ok(true)
}

fn test_journal_error() -> EnsureError {
    EnsureError::Unexpected {
        status: 0,
        step: "test-journal",
    }
}

fn batch_message(polled: &Poll) -> Option<i64> {
    match polled {
        Poll::Batch(batch) => Some(batch.message_id),
        Poll::Empty => None,
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

fn completion_poll(request_id: i64) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 200 + request_id,
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Completed,
            request_id: Some(request_id),
            context: empty_context(),
            runner_id: Some(71),
            runner_name: None,
            result: Some("Succeeded".to_owned()),
            fields: Vec::new(),
        }],
    })
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

async fn open_journal(scratch: &Scratch) -> Result<Journal, String> {
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("docker-engine-test")
        .await
        .map_err(|error| error.to_string())?;
    Ok(journal)
}

#[tokio::test]
async fn listener_session_refills_capacity_one_after_completion_and_clean_failure()
-> Result<(), String> {
    let _capacity = install_job_capacity(CAPACITY);
    let scratch = Scratch::new("poll-capacity-one-waves").map_err(|error| error.to_string())?;
    let journal = open_journal(&scratch).await?;
    let polls = VecDeque::from([
        available(&[42]),
        completion_poll(42),
        available(&[43]),
        completion_poll(43),
        assigned_wait(101, 1),
        available(&[44]),
        completion_poll(44),
        Poll::Empty,
        Poll::Empty,
    ]);
    let mut session = ListenerSession {
        journal,
        polls,
        dispatcher: Waves::new(101),
        running: 0,
        seen: 0,
        cleanups: Vec::new(),
    };
    let mut workers = Vec::new();

    until_idle(&mut session, &mut workers, 1)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(session.seen, 9);
    assert_eq!(workers.len(), 3);
    assert_eq!(session.dispatcher.start_count, 4);
    assert_eq!(session.running, 0);
    assert_eq!(session.journal.occupied_launches().await, Ok(0));
    assert_eq!(
        session.dispatcher.script.calls,
        [
            "acquire", "jit", "ack", "ack", "acquire", "jit", "ack", "ack", "ack", "acquire",
            "jit", "ack", "ack",
        ]
    );
    Ok(())
}
