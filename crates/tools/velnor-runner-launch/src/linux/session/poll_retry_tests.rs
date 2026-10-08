use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::sleep;
use velnor_runner_github::policy::PollWithTrust;
use velnor_runner_github::{RefreshGate, SessionError, StatusClass, WireError, classify_status};
use velnor_runner_host::Journal;

use super::poll_retry::{
    PollAttempt, PollFuture, PollRetryIo, WaitFuture, classify_poll_result, poll_until_delivery,
    wait_before_next_poll,
};
use crate::launch::harness::Scratch;

struct ScriptedPollIo {
    polls: VecDeque<ScriptedPoll>,
    cursor_reads: Vec<i64>,
    waits: Vec<Duration>,
}

enum ScriptedPoll {
    Result(Result<PollWithTrust, SessionError>),
    AfterRefresh(Result<PollWithTrust, SessionError>),
}

fn classify_scripted_poll(script: Option<ScriptedPoll>) -> PollAttempt {
    match script {
        Some(ScriptedPoll::Result(result)) => classify_poll_result(result, &RefreshGate::new()),
        Some(ScriptedPoll::AfterRefresh(result)) => {
            let gate = RefreshGate::new();
            assert_eq!(classify_status(401, &gate), Ok(StatusClass::RefreshOnce));
            classify_poll_result(result, &gate)
        }
        None => PollAttempt::Terminal,
    }
}

impl ScriptedPollIo {
    fn new(polls: impl IntoIterator<Item = ScriptedPoll>) -> Self {
        Self {
            polls: polls.into_iter().collect(),
            cursor_reads: Vec::new(),
            waits: Vec::new(),
        }
    }
}

impl PollRetryIo for ScriptedPollIo {
    fn poll(&mut self, cursor: i64) -> PollFuture<'_> {
        self.cursor_reads.push(cursor);
        let attempt = classify_scripted_poll(self.polls.pop_front());
        Box::pin(async move { attempt })
    }

    fn wait(&mut self, delay: Duration) -> WaitFuture<'_> {
        self.waits.push(delay);
        Box::pin(async { true })
    }
}

struct ShutdownPollIo {
    journal: Journal,
    receiver: watch::Receiver<Option<Instant>>,
    cutoff: Option<Instant>,
    polls: VecDeque<ScriptedPoll>,
    cursor_reads: Vec<i64>,
    waits: Vec<Duration>,
}

impl PollRetryIo for ShutdownPollIo {
    fn poll(&mut self, cursor: i64) -> PollFuture<'_> {
        self.cursor_reads.push(cursor);
        let attempt = classify_scripted_poll(self.polls.pop_front());
        Box::pin(async move { attempt })
    }

    fn wait(&mut self, delay: Duration) -> WaitFuture<'_> {
        self.waits.push(delay);
        Box::pin(wait_before_next_poll(
            &self.journal,
            &mut self.receiver,
            &mut self.cutoff,
            Duration::from_secs(2),
            delay,
        ))
    }
}

#[tokio::test]
async fn uncertain_poll_retries_same_cursor_then_returns_delivery() {
    let mut io = ScriptedPollIo::new([
        ScriptedPoll::Result(Err(SessionError::Uncertain)),
        ScriptedPoll::Result(Ok(PollWithTrust::Empty)),
    ]);

    let result = poll_until_delivery(&mut io, 17).await;

    assert_eq!(result, Some(PollWithTrust::Empty));
    assert_eq!(io.cursor_reads, [17, 17]);
    assert_eq!(io.waits, [Duration::from_millis(250)]);
}

#[tokio::test]
async fn terminal_poll_results_do_not_dispatch_another_get() {
    for terminal in [
        ScriptedPoll::Result(Err(SessionError::Wire(WireError::Malformed))),
        ScriptedPoll::AfterRefresh(Err(SessionError::Uncertain)),
    ] {
        let mut io =
            ScriptedPollIo::new([terminal, ScriptedPoll::Result(Ok(PollWithTrust::Empty))]);

        let result = poll_until_delivery(&mut io, 22).await;

        assert_eq!(result, None);
        assert_eq!(io.cursor_reads, [22]);
        assert_eq!(io.waits, Vec::<Duration>::new());
    }
}

#[tokio::test]
async fn retry_budget_caps_at_initial_poll_plus_three_retries() {
    let mut io = ScriptedPollIo::new(
        std::iter::repeat_with(|| ScriptedPoll::Result(Err(SessionError::Uncertain))).take(4),
    );

    let result = poll_until_delivery(&mut io, 29).await;

    assert_eq!(result, None);
    assert_eq!(io.cursor_reads, [29, 29, 29, 29]);
    assert_eq!(
        io.waits,
        [
            Duration::from_millis(250),
            Duration::from_millis(500),
            Duration::from_secs(1),
        ]
    );
}

#[tokio::test]
async fn shutdown_during_retry_wait_persists_fence_without_another_get() {
    let scratch = Scratch::new("linux-poll-retry-shutdown").expect("scratch directory");
    let journal = Journal::open(&scratch.file()).await.expect("journal opens");
    let (sender, receiver) = watch::channel(None);
    let mut io = ShutdownPollIo {
        journal,
        receiver,
        cutoff: None,
        polls: VecDeque::from([
            ScriptedPoll::Result(Err(SessionError::Uncertain)),
            ScriptedPoll::Result(Ok(PollWithTrust::Empty)),
        ]),
        cursor_reads: Vec::new(),
        waits: Vec::new(),
    };
    let stop_at = Instant::now()
        .checked_add(Duration::from_millis(100))
        .expect("monotonic clock supports cutoff");
    let mut driver = Box::pin(poll_until_delivery(&mut io, 31));
    let early = tokio::select! {
        result = &mut driver => Some(result),
        () = sleep(Duration::from_millis(20)) => None,
    };
    assert!(early.is_none(), "driver should be in the retry wait");
    sender
        .send(Some(stop_at))
        .expect("wait still observes shutdown");

    assert_eq!(driver.await, None);
    assert_eq!(io.cursor_reads, [31]);
    assert_eq!(io.waits, [Duration::from_millis(250)]);
    assert_eq!(io.cutoff, Some(stop_at));
    assert!(io.journal.draining().await.expect("drain state reads"));
}
