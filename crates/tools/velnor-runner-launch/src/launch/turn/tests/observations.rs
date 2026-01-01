//! Queue lifecycle events remain durable even when an offer is held.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use velnor_runner_github::{
    AsyncDiscoveryTransport, DiscoveryExchange, Exchange, InnerJob, InnerKind, ParsedBatch, Poll,
    SessionError, SessionRequest, TransportFail,
};
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_host::worker::Started;
use velnor_runner_journal::journal::Outcome;

use crate::launch::capacity::Admit;
use crate::launch::harness::open;
use crate::launch::turn::{
    HeldOffers, PollHost, ReconciliationBudget, admission, persist_and_reconcile_observations, pump,
};

#[tokio::test]
async fn held_message_retries_actual_started_identity_in_same_pump() -> Result<(), String> {
    let (_scratch, journal) = open("poll-observations").await?;
    let launch_id = started_launch(&journal, "held-offer", 'a', 'b').await?;

    let mut host = HeldReconcilePolls {
        journal: &journal,
        actions: ActionsTransport::new(vec![
            workflow_run_in_progress(),
            in_progress_jobs(),
            workflow_run(),
            completed_jobs(),
        ]),
        polls: VecDeque::from([held_offer_with_started(), held_offer_with_started()]),
        pending: false,
        poll_count: 0,
        message_ids: Vec::new(),
        decisions: Vec::new(),
        held_offers: HeldOffers::default(),
        session_id: "same-queue-session",
        budget: ReconciliationBudget::default(),
        running_until: 0,
    };
    let mut workers = Vec::new();
    pump(&mut host, &mut workers, 1)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(host.poll_count, 2);
    assert_eq!(host.message_ids, [102, 102]);
    assert_eq!(host.decisions, [Admit::Hold, Admit::Hold]);
    assert_eq!(host.session_id, "same-queue-session");
    assert!(host.held_offers.requires_retention());
    assert_eq!(workers.len(), 0);

    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "observed launch row disappeared".to_owned())?;
    assert_eq!(row.github_runner_id.as_deref(), Some("81"));
    assert_eq!(row.observed_job_id.as_deref(), Some("scale-job-opaque"));
    assert_eq!(row.observed_workflow_run_id, Some(9001));
    assert_eq!(row.observed_actions_attempt, Some(1));
    assert_eq!(row.observed_actions_job_id, Some(777));
    assert_eq!(row.observed_actions_conclusion.as_deref(), Some("failure"));
    assert!(row.remote_terminal);
    assert!(!row.cleanup_proven);
    assert_eq!(velnor_runner_launch_slot::occupied(&journal).await, Ok(1));

    assert_actions_requests(&host.actions)?;
    Ok(())
}

fn held_offer_with_started() -> Poll {
    Poll::Batch(ParsedBatch {
        message_id: 102,
        statistics: None,
        jobs: vec![
            InnerJob {
                kind: InnerKind::Available,
                request_id: Some(999),
                job_id: Some("available-opaque".to_owned()),
                ..lifecycle_event(InnerKind::Available)
            },
            lifecycle_event(InnerKind::Started),
        ],
    })
}

#[tokio::test]
async fn expired_reconciliation_budget_retains_offer_without_ack_or_release() -> Result<(), String>
{
    let (_scratch, journal) = open("poll-observations-expired").await?;
    let launch_id = started_launch(&journal, "held-offer-expired", 'e', 'f').await?;
    let mut host = HeldReconcilePolls {
        journal: &journal,
        actions: ActionsTransport::new(vec![workflow_run_in_progress(), in_progress_jobs()]),
        polls: VecDeque::from([held_offer_with_started()]),
        pending: false,
        poll_count: 0,
        message_ids: Vec::new(),
        decisions: Vec::new(),
        held_offers: HeldOffers::default(),
        session_id: "same-queue-session",
        budget: ReconciliationBudget {
            deadline: Some(tokio::time::Instant::now()),
            attempts: 0,
        },
        running_until: 0,
    };
    let mut workers = Vec::new();
    pump(&mut host, &mut workers, 1)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(host.poll_count, 1);
    assert_eq!(host.message_ids, [102]);
    assert_eq!(host.decisions, [Admit::Hold]);
    assert!(host.held_offers.requires_retention());
    assert_eq!(
        host.actions
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        0
    );
    assert_eq!(workers.len(), 0);
    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "uncertain launch row disappeared".to_owned())?;
    assert!(!row.remote_terminal);
    assert!(!row.cleanup_proven);
    assert_eq!(velnor_runner_launch_slot::occupied(&journal).await, Ok(1));
    Ok(())
}

#[tokio::test]
async fn running_worker_keeps_lifecycle_polls_but_not_rest_retries_past_budget()
-> Result<(), String> {
    let (_scratch, journal) = open("poll-observations-running-budget").await?;
    let launch_id = started_launch(&journal, "held-offer-running", '9', 'a').await?;
    let actions = std::iter::repeat_with(|| [workflow_run_in_progress(), in_progress_jobs()])
        .take(8)
        .flatten()
        .collect();
    let mut host = HeldReconcilePolls {
        journal: &journal,
        actions: ActionsTransport::new(actions),
        polls: std::iter::repeat_with(held_offer_with_started)
            .take(10)
            .collect(),
        pending: false,
        poll_count: 0,
        message_ids: Vec::new(),
        decisions: Vec::new(),
        held_offers: HeldOffers::default(),
        session_id: "same-queue-session",
        budget: ReconciliationBudget::default(),
        running_until: 10,
    };
    let mut workers = Vec::new();
    pump(&mut host, &mut workers, 1)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(host.poll_count, 10);
    assert!(host.message_ids.iter().all(|message_id| *message_id == 102));
    assert!(host.held_offers.requires_retention());
    assert_eq!(host.budget.attempts, 8);
    assert_eq!(
        host.actions
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        16
    );
    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "running launch row disappeared".to_owned())?;
    assert!(!row.remote_terminal);
    assert!(!row.cleanup_proven);
    assert_eq!(velnor_runner_launch_slot::occupied(&journal).await, Ok(1));
    Ok(())
}

#[tokio::test]
async fn running_worker_does_not_extend_budget_after_unknown_rest_failures() -> Result<(), String> {
    let (_scratch, journal) = open("poll-observations-running-unknown").await?;
    let launch_id = started_launch(&journal, "held-offer-running-unknown", 'b', 'c').await?;
    let mut host = HeldReconcilePolls {
        journal: &journal,
        actions: ActionsTransport::new(
            (0..8)
                .map(|cycle| rest_failure(if cycle % 2 == 0 { 401 } else { 503 }))
                .collect(),
        ),
        polls: std::iter::repeat_with(held_offer_with_started)
            .take(10)
            .collect(),
        pending: false,
        poll_count: 0,
        message_ids: Vec::new(),
        decisions: Vec::new(),
        held_offers: HeldOffers::default(),
        session_id: "same-queue-session",
        budget: ReconciliationBudget::default(),
        running_until: 10,
    };
    let mut workers = Vec::new();
    pump(&mut host, &mut workers, 1)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(host.poll_count, 10);
    assert_eq!(host.budget.attempts, 8);
    assert!(host.held_offers.requires_retention());
    assert_eq!(
        host.actions
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        8
    );
    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "unknown reconciliation row disappeared".to_owned())?;
    assert!(!row.remote_terminal);
    assert!(!row.cleanup_proven);
    assert_eq!(velnor_runner_launch_slot::occupied(&journal).await, Ok(1));
    Ok(())
}

#[test]
fn reconciliation_retry_budget_requires_pending_work_before_deadline_and_below_poll_cap() {
    let now = tokio::time::Instant::now();
    let deadline = Some(now + std::time::Duration::from_secs(1));
    assert!(super::super::retry_window_open(true, 0, deadline, now));
    assert!(!super::super::retry_window_open(false, 0, deadline, now));
    assert!(!super::super::retry_window_open(true, 8, deadline, now));
    assert!(!super::super::retry_window_open(true, 0, Some(now), now));
}

#[tokio::test]
async fn unmatched_rest_runner_does_not_persist_terminal_evidence() -> Result<(), String> {
    let (_scratch, journal) = open("poll-observations-rest-mismatch").await?;
    let launch_id = started_launch(&journal, "held-offer-mismatch", 'c', 'd').await?;

    let mut transport = ActionsTransport::new(vec![workflow_run(), unrelated_runner_jobs()]);
    let poll = Poll::Batch(ParsedBatch {
        message_id: 104,
        statistics: None,
        jobs: vec![lifecycle_event(InnerKind::Started)],
    });
    persist(&journal, &poll, &mut transport).await?;

    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "observed launch row disappeared".to_owned())?;
    assert_eq!(row.github_runner_id.as_deref(), Some("81"));
    assert!(!row.remote_terminal);
    assert_eq!(row.observed_actions_attempt, None);
    assert_eq!(row.observed_actions_job_id, None);
    assert!(!row.cleanup_proven);
    Ok(())
}

async fn started_launch(
    journal: &velnor_runner_journal::journal::Journal,
    subject: &str,
    runner_seed: char,
    dind_seed: char,
) -> Result<i64, String> {
    let (launch_id, fresh) = journal
        .begin_launch(subject)
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    journal
        .bind_launch_identity(launch_id, None, None, None, None, "runner-actual")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_launch_effect_intent(launch_id)
        .await
        .map_err(|error| error.to_string())?;
    let runner_container = runner_seed.to_string().repeat(64);
    let dind_container = dind_seed.to_string().repeat(64);
    journal
        .bind(launch_id, Some(&runner_container), Some("81"))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(launch_id, Some(&runner_container), Some(&dind_container))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_runner_start_intent(launch_id, &runner_container)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(launch_id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    Ok(launch_id)
}

async fn persist(
    journal: &velnor_runner_journal::journal::Journal,
    poll: &Poll,
    transport: &mut ActionsTransport,
) -> Result<bool, String> {
    super::super::observations::persist_observed_lifecycle(
        journal,
        poll,
        transport,
        "acme",
        "runner",
        "actions-read-token",
        tokio::time::Instant::now() + std::time::Duration::from_secs(30),
    )
    .await
    .map_err(|error| error.to_string())
}

struct HeldReconcilePolls<'a> {
    journal: &'a velnor_runner_journal::journal::Journal,
    actions: ActionsTransport,
    polls: VecDeque<Poll>,
    pending: bool,
    poll_count: usize,
    message_ids: Vec<i64>,
    decisions: Vec<Admit>,
    held_offers: HeldOffers,
    session_id: &'static str,
    budget: ReconciliationBudget,
    running_until: usize,
}

impl PollHost for HeldReconcilePolls<'_> {
    async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        if !workers.is_empty() {
            return Err(EnsureError::Unexpected {
                status: 0,
                step: "test workers",
            });
        }
        let Some(polled) = self.polls.pop_front() else {
            return Err(EnsureError::Unexpected {
                status: 0,
                step: "test poll exhaustion",
            });
        };
        self.poll_count = self.poll_count.saturating_add(1);
        if let Poll::Batch(batch) = &polled {
            self.message_ids.push(batch.message_id);
        }
        self.held_offers.observe_batch(&polled);
        self.pending = persist_and_reconcile_observations(
            self.journal,
            &polled,
            &mut self.actions,
            "acme",
            "runner",
            "actions-read-token",
            &mut self.budget,
        )
        .await?;
        let decision = admission(
            &crate::launch::fakes::Engine::new(),
            self.journal,
            1,
            1,
            0,
            &polled,
        )
        .await?;
        self.decisions.push(decision);
        if decision == Admit::Hold {
            // Hold leaves the whole message unacknowledged, so the queue
            // returns it again with the same session delivery context.
            self.held_offers.observe_ack(None);
            Ok(true)
        } else {
            Err(EnsureError::Unexpected {
                status: 0,
                step: "test expected held offer",
            })
        }
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "the fake running count is an in-memory value"
    )]
    async fn running(&mut self) -> Result<u32, EnsureError> {
        Ok(u32::from(self.poll_count < self.running_until))
    }

    async fn pause(&mut self, _duration: std::time::Duration) {
        tokio::task::yield_now().await;
    }

    fn reconciliation_pending(&self) -> bool {
        self.pending
    }

    fn reconciliation_retry_allowed(&self) -> bool {
        self.budget
            .retry_allowed(self.pending, tokio::time::Instant::now())
    }
}

fn assert_actions_requests(transport: &ActionsTransport) -> Result<(), String> {
    let requests = transport
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].path, "repos/acme/runner/actions/runs/9001");
    assert_eq!(
        requests[1].path,
        "repos/acme/runner/actions/runs/9001/attempts/1/jobs"
    );
    assert_eq!(requests[2].path, "repos/acme/runner/actions/runs/9001");
    assert_eq!(
        requests[3].path,
        "repos/acme/runner/actions/runs/9001/attempts/1/jobs"
    );
    assert!(
        requests
            .iter()
            .all(|request| request.headers.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("authorization") && value == "Bearer actions-read-token"
            }))
    );
    Ok(())
}

struct ActionsTransport {
    requests: Arc<Mutex<Vec<SessionRequest>>>,
    responses: VecDeque<Exchange>,
    origin_binds: Arc<AtomicUsize>,
}

impl ActionsTransport {
    fn new(responses: Vec<Exchange>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            responses: responses.into(),
            origin_binds: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl AsyncDiscoveryTransport for ActionsTransport {
    fn bind_github_api_origin(&mut self) -> Result<(), SessionError> {
        self.origin_binds.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    fn bind_actions_service_origin(&mut self, _url: &str) -> Result<(), SessionError> {
        Err(velnor_runner_github::WireError::RegistrationRejected.into())
    }

    fn exchange_discovery(&mut self, request: SessionRequest) -> DiscoveryExchange {
        self.requests
            .lock()
            .expect("test transport mutex")
            .push(request);
        let response = self.responses.pop_front().ok_or(TransportFail::Http(500));
        DiscoveryExchange::new(async move { response }, Arc::new(AtomicBool::new(false)))
    }
}

fn workflow_run() -> Exchange {
    workflow_run_with_status("completed")
}

fn workflow_run_in_progress() -> Exchange {
    workflow_run_with_status("in_progress")
}

fn workflow_run_with_status(status: &str) -> Exchange {
    json_exchange(&serde_json::json!({
        "id": 9001,
        "path": ".github/workflows/test.yml",
        "run_attempt": 1,
        "status": status,
        "conclusion": "failure",
        "event": "push",
        "head_sha": "0123456789abcdef",
        "head_repository": { "full_name": "acme/runner" }
    }))
}

fn in_progress_jobs() -> Exchange {
    json_exchange(&serde_json::json!({
        "total_count": 1,
        "jobs": [{
            "id": 777,
            "run_id": 9001,
            "status": "in_progress",
            "conclusion": null,
            "runner_id": 81,
            "runner_name": "runner-actual"
        }]
    }))
}

fn completed_jobs() -> Exchange {
    json_exchange(&serde_json::json!({
        "total_count": 1,
        "jobs": [{
            "id": 777,
            "run_id": 9001,
            "status": "completed",
            "conclusion": "failure",
            "runner_id": 81,
            "runner_name": "runner-actual"
        }]
    }))
}

fn unrelated_runner_jobs() -> Exchange {
    json_exchange(&serde_json::json!({
        "total_count": 1,
        "jobs": [{
            "id": 778,
            "run_id": 9001,
            "status": "completed",
            "conclusion": "success",
            "runner_id": 82,
            "runner_name": "another-runner"
        }]
    }))
}

fn json_exchange(value: &serde_json::Value) -> Exchange {
    Exchange {
        status: 200,
        body: value.to_string().into_bytes(),
    }
}

fn rest_failure(status: u16) -> Exchange {
    Exchange {
        status,
        body: Vec::new(),
    }
}

fn lifecycle_event(kind: InnerKind) -> InnerJob {
    InnerJob {
        kind,
        request_id: None,
        job_id: Some("scale-job-opaque".to_owned()),
        workflow_run_id: Some(9001),
        owner_name: None,
        repository_name: None,
        event_name: None,
        labels: Vec::new(),
        runner_id: Some(81),
        runner_name: Some("runner-actual".to_owned()),
        result: None,
        fields: Vec::new(),
    }
}
