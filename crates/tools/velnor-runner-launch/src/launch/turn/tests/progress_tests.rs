//! Missing-statistics batches ack and continue through the real pump.

use std::collections::VecDeque;

use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};

use super::super::{PollHost, admission, pump};
use crate::launch;
use crate::launch::capacity::Admit;
use crate::launch::harness::{Mode, Script, absent, ctx, open};
use crate::launch::steps;
use velnor_runner_host::journal::Outcome;
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_host::worker::Started;

struct ProgressPolls<'a> {
    journal: &'a velnor_runner_host::Journal,
    polls: VecDeque<Poll>,
    poll_count: usize,
    script: Script,
}

impl PollHost for ProgressPolls<'_> {
    async fn poll(&mut self, _workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        self.poll_count = self.poll_count.saturating_add(1);
        let Some(polled) = self.polls.pop_front() else {
            return Ok(true);
        };
        let decision = admission(
            &crate::launch::fakes::Engine::new(),
            self.journal,
            2,
            2,
            0,
            &polled,
        )
        .await?;
        let Admit::Ack { stop } = decision else {
            return Err(EnsureError::Unexpected {
                status: 0,
                step: "queue",
            });
        };
        if let Poll::Batch(batch) = &polled {
            steps::acknowledge(&mut self.script, &ctx(), batch)?;
        }
        Ok(stop)
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "the fake runner count is an in-memory value"
    )]
    async fn running(&mut self) -> Result<u32, EnsureError> {
        Ok(0)
    }
}

#[tokio::test]
async fn missing_stats_notifications_ack_then_poll_again() -> Result<(), String> {
    let (scratch, journal) = open("missing-stats-pump").await?;
    let (row, _) = journal
        .begin_launch("m95")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(row, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(launch::slot::occupied(&journal).await, Ok(1));

    let mut host = ProgressPolls {
        journal: &journal,
        polls: VecDeque::from([
            batch(96, Vec::new()),
            batch(97, vec![job(InnerKind::Started), job(InnerKind::Completed)]),
        ]),
        poll_count: 0,
        script: Script {
            calls: Vec::new(),
            mode: Mode::Ok,
        },
    };
    let mut workers = Vec::new();
    pump(&mut host, &mut workers, 1)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(host.poll_count, 3);
    assert_eq!(host.script.calls, ["ack", "ack"]);
    assert_eq!(workers, Vec::<Started>::new());
    assert_eq!(launch::slot::occupied(&journal).await, Ok(1));
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, row);
    assert_eq!(rows[0].subject, "m95");
    assert_eq!(rows[0].state, velnor_runner_host::IntentState::Uncertain);
    absent(&scratch.file())
}

#[tokio::test]
async fn ack_error_propagates_without_start_or_repoll() -> Result<(), String> {
    let (scratch, journal) = open("missing-stats-ack-fail").await?;
    let mut host = ProgressPolls {
        journal: &journal,
        polls: VecDeque::from([batch(98, Vec::new())]),
        poll_count: 0,
        script: Script {
            calls: Vec::new(),
            mode: Mode::AckFail,
        },
    };
    let mut workers = Vec::new();
    assert_eq!(
        pump(&mut host, &mut workers, 1).await,
        Err(EnsureError::Uncertain)
    );
    assert_eq!(host.poll_count, 1);
    assert_eq!(host.script.calls, ["ack"]);
    assert_eq!(workers, Vec::<Started>::new());
    assert_eq!(launch::slot::occupied(&journal).await, Ok(0));
    absent(&scratch.file())
}

fn batch(message_id: i64, jobs: Vec<InnerJob>) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id,
        statistics: None,
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
