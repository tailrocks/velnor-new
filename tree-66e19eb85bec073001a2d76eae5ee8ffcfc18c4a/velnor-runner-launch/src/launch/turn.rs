//! One session loop. A running owned worker keeps the session up.

use std::{collections::BTreeSet, time::Duration};

use velnor_runner_github::{Poll, QueueSession};

use velnor_runner_host::BoundedDiscoveryTransport;
use velnor_runner_host::listen::{Link, point_at_queue, poll_path, restore_base};
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_host::worker::Started;
use velnor_runner_journal::journal::Journal;

use super::capacity::{self, Admit};
use super::steps;
use super::trace;
use super::{Ready, ack_ready, drive_ready, scale_session};
use velnor_runner_launch_slot as slot;

const RECONCILIATION_WINDOW: Duration = Duration::from_secs(120);
const RECONCILIATION_POLL_BUDGET: Duration = Duration::from_secs(30);
const MAX_RECONCILIATION_POLLS: u8 = 8;

/// Poll until admission stops and no owned launch container is running.
///
/// # Errors
///
/// Returns [`EnsureError`] when one message carries two ids, or a later step fails.
pub(super) async fn poll_and_drive<'turn>(
    link: &'turn mut Link,
    set_id: i64,
    session: &'turn mut QueueSession,
    admin_token: &'turn str,
    actions_reconciler: &'turn mut ActionsReconciler<'_>,
    journal: &'turn Journal,
    docker: &'turn bollard::Docker,
) -> Result<PollOutcome, EnsureError> {
    trace::session(session);
    let mut workers = Vec::new();
    let capacity = capacity::job_capacity();
    let population = session
        .statistics()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    let occupied = slot::occupied(journal).await?;
    let running = slot::running_count(journal, docker).await?;
    if !capacity::statistics_blocked(occupied, running, capacity, population)
        && let Some(started) =
            scale_session(link, set_id, session, admin_token, journal, docker).await?
    {
        workers.push(started);
    }
    let target = capacity::admit_target(capacity);
    let mut turn = Turn {
        link,
        set_id,
        session,
        admin_token,
        actions_reconciler,
        journal,
        docker,
        capacity,
        target,
        last_message_id: 0,
        held_offers: HeldOffers::default(),
        reconciliation_pending: false,
        reconciliation: ReconciliationBudget::default(),
    };
    let bound = if target > capacity {
        capacity::poll_bound_wide()
    } else {
        capacity::poll_bound(capacity)
    };
    until_idle(&mut turn, &mut workers, bound).await?;
    Ok(PollOutcome {
        workers,
        retain_session: turn.held_offers.requires_retention(),
    })
}

/// Result of polling, including whether an unacknowledged offer still needs this session.
pub(super) struct PollOutcome {
    pub(super) workers: Vec<Started>,
    pub(super) retain_session: bool,
}

#[derive(Default)]
struct HeldOffers {
    message_ids: BTreeSet<i64>,
}

impl HeldOffers {
    fn observe_batch(&mut self, polled: &Poll) {
        if let Poll::Batch(batch) = polled {
            self.message_ids.insert(batch.message_id);
        }
    }

    fn observe_ack(&mut self, message_id: Option<i64>) {
        if let Some(message_id) = message_id {
            self.message_ids.remove(&message_id);
        }
    }

    fn requires_retention(&self) -> bool {
        !self.message_ids.is_empty()
    }
}

/// One session's poll and running-count source.
trait PollHost {
    /// `Ok(false)` keeps the session. `Ok(true)` is an admission stop.
    async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError>;

    /// Owned containers still running.
    async fn running(&mut self) -> Result<u32, EnsureError>;

    /// Wait before another queue poll.
    async fn pause(&mut self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }

    /// Whether the last held poll still has an actual Started job to reconcile.
    fn reconciliation_pending(&self) -> bool {
        false
    }

    /// Whether another bounded read-only poll is still allowed.
    fn reconciliation_retry_allowed(&self) -> bool {
        self.reconciliation_pending()
    }
}

impl PollHost for Turn<'_, '_> {
    async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        self.drive_poll(workers).await
    }

    async fn running(&mut self) -> Result<u32, EnsureError> {
        slot::running_count(self.journal, self.docker).await
    }

    fn reconciliation_pending(&self) -> bool {
        self.reconciliation_pending
    }

    fn reconciliation_retry_allowed(&self) -> bool {
        self.reconciliation
            .retry_allowed(self.reconciliation_pending, tokio::time::Instant::now())
    }
}

/// Keep polling while an owned container runs. An empty session stays open past `bound`.
async fn until_idle(
    turn: &mut Turn<'_, '_>,
    workers: &mut Vec<Started>,
    bound: usize,
) -> Result<(), EnsureError> {
    pump(turn, workers, bound).await
}

async fn pump<H: PollHost>(
    host: &mut H,
    workers: &mut Vec<Started>,
    bound: usize,
) -> Result<(), EnsureError> {
    let mut polls = 0usize;
    let mut missed = 0u8;
    loop {
        if polls >= bound && !workers.is_empty() && missed >= 2 && host.running().await? == 0 {
            return Ok(());
        }
        let stop = host.poll(workers).await?;
        polls = polls.saturating_add(1);
        // A pending reconciliation has its own fixed GET budget. A running
        // worker may keep ordinary lifecycle polling alive, but it cannot
        // bypass that budget by taking either the Stay or Hold branch.
        if host.reconciliation_pending()
            && !host.reconciliation_retry_allowed()
            && host.running().await? == 0
        {
            return Ok(());
        }
        if !stop {
            // The broker can assign a job only while this session still exists.
            if workers.is_empty() {
                host.pause(Duration::from_secs(1)).await;
            }
            continue;
        }
        if host.running().await? > 0 {
            missed = 0;
            host.pause(Duration::from_secs(2)).await;
            continue;
        }
        if host.reconciliation_pending() {
            // Keep this same session and message open while a bounded REST
            // lookup is nonterminal. The next poll may redeliver this message;
            // it must not be ACKed merely to advance the queue cursor. Stop at
            // the fixed retry window so a failed/unsupported route cannot loop
            // forever; the caller still retains the unacknowledged offer.
            if !host.reconciliation_retry_allowed() {
                return Ok(());
            }
            missed = 0;
            host.pause(Duration::from_secs(2)).await;
            continue;
        }
        missed = missed.saturating_add(1);
        if workers.is_empty() || missed >= 2 {
            return Ok(());
        }
        host.pause(Duration::from_secs(2)).await;
    }
}

fn retry_window_open(
    pending: bool,
    polls: u8,
    deadline: Option<tokio::time::Instant>,
    now: tokio::time::Instant,
) -> bool {
    pending && polls < MAX_RECONCILIATION_POLLS && deadline.is_some_and(|deadline| now < deadline)
}

#[derive(Default)]
struct ReconciliationBudget {
    deadline: Option<tokio::time::Instant>,
    attempts: u8,
}

impl ReconciliationBudget {
    fn begin_attempt(&mut self, now: tokio::time::Instant) -> Option<tokio::time::Instant> {
        let deadline = *self
            .deadline
            .get_or_insert_with(|| now + RECONCILIATION_WINDOW);
        if self.attempts >= MAX_RECONCILIATION_POLLS || now >= deadline {
            return None;
        }
        self.attempts = self.attempts.saturating_add(1);
        Some(std::cmp::min(deadline, now + RECONCILIATION_POLL_BUDGET))
    }

    fn retry_allowed(&self, pending: bool, now: tokio::time::Instant) -> bool {
        retry_window_open(pending, self.attempts, self.deadline, now)
    }

    fn clear(&mut self) {
        self.deadline = None;
        self.attempts = 0;
    }
}

async fn persist_and_reconcile_observations(
    journal: &Journal,
    polled: &Poll,
    transport: &mut (impl velnor_runner_github::AsyncDiscoveryTransport + ?Sized),
    owner: &str,
    repository: &str,
    actions_token: &str,
    budget: &mut ReconciliationBudget,
) -> Result<bool, EnsureError> {
    observations::persist_lifecycle_events(journal, polled).await?;
    if !observations::has_pending_actions_reconciliation(journal).await? {
        budget.clear();
        return Ok(false);
    }

    let Some(deadline) = budget.begin_attempt(tokio::time::Instant::now()) else {
        // Keep the actual event and unresolved launch durable, but do not send
        // another Actions request after the cycle or wall-clock budget closes.
        return Ok(true);
    };
    let pending = observations::reconcile_pending_lifecycle(
        journal,
        transport,
        owner,
        repository,
        actions_token,
        deadline,
    )
    .await?;
    if !pending {
        budget.clear();
    }
    Ok(pending)
}

fn progress_batch(polled: &Poll) -> bool {
    let Poll::Batch(batch) = polled else {
        return false;
    };
    velnor_runner_host::assign::progress_only(batch)
}

fn assigned_in(polled: &Poll) -> u32 {
    let Poll::Batch(batch) = polled else {
        return 0;
    };
    let raw = batch
        .statistics
        .as_ref()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    u32::try_from(raw.max(0)).unwrap_or(u32::MAX)
}

/// Decide one poll: start, hold, ack, or stop. Cleanup must be proven separately.
///
/// # Errors
///
/// Returns [`EnsureError`] when the journal or engine fails mid-decision;
/// the launch stays unacquired so the next poll retries.
pub async fn admission<E: velnor_runner_host::stage::PairEngine + ?Sized>(
    engine: &E,
    journal: &Journal,
    capacity: u32,
    target: u32,
    started: u32,
    polled: &Poll,
) -> Result<Admit, EnsureError> {
    let idle = steps::idle(polled);
    let occupied = slot::occupied(journal).await?;
    let running = if capacity::needs_running(idle) {
        slot::running_count(journal, engine).await?
    } else {
        0
    };
    Ok(capacity::admit(capacity::Seat {
        capacity,
        target,
        started,
        occupied,
        running,
        assigned: assigned_in(polled),
        idle,
        progress: progress_batch(polled),
    }))
}

struct Turn<'turn, 'context> {
    link: &'turn mut Link,
    set_id: i64,
    session: &'turn mut QueueSession,
    admin_token: &'turn str,
    actions_reconciler: &'turn mut ActionsReconciler<'context>,
    journal: &'turn Journal,
    docker: &'turn bollard::Docker,
    capacity: u32,
    target: u32,
    last_message_id: i64,
    held_offers: HeldOffers,
    reconciliation_pending: bool,
    reconciliation: ReconciliationBudget,
}

pub(super) struct ActionsReconciler<'a> {
    owner: &'a str,
    repository: &'a str,
    actions_token: &'a str,
    transport: &'a mut BoundedDiscoveryTransport,
}

impl<'a> ActionsReconciler<'a> {
    pub(super) fn new(
        owner: &'a str,
        repository: &'a str,
        actions_token: &'a str,
        transport: &'a mut BoundedDiscoveryTransport,
    ) -> Self {
        Self {
            owner,
            repository,
            actions_token,
            transport,
        }
    }
}

impl Turn<'_, '_> {
    async fn drive_poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        let admin = self.link.base().to_owned();
        let polled = poll_path(
            self.link,
            self.session,
            self.set_id,
            self.admin_token,
            self.last_message_id,
            self.capacity,
        );
        let polled = polled?;
        let (saved, path) = point_at_queue(self.link, &self.session.message_queue_url)?;
        let queue = saved.as_ref().map(|_| self.link.base().to_owned());
        restore_base(self.link, saved)?;
        if self.link.base() != admin {
            return Err(EnsureError::Endpoint);
        }
        trace::batch(&polled);
        self.reconciliation_pending = persist_and_reconcile_observations(
            self.journal,
            &polled,
            self.actions_reconciler.transport,
            self.actions_reconciler.owner,
            self.actions_reconciler.repository,
            self.actions_reconciler.actions_token,
            &mut self.reconciliation,
        )
        .await?;
        let started = u32::try_from(workers.len()).unwrap_or(u32::MAX);
        let decision = admission(
            self.docker,
            self.journal,
            self.capacity,
            self.target,
            started,
            &polled,
        )
        .await?;
        self.apply(decision, workers, path, queue, &polled).await
    }

    async fn apply(
        &mut self,
        decision: Admit,
        workers: &mut Vec<Started>,
        path: String,
        queue: Option<String>,
        polled: &Poll,
    ) -> Result<bool, EnsureError> {
        // Any real message remains attached to this session until its exact
        // id receives a successful DELETE, regardless of the admission branch.
        self.held_offers.observe_batch(polled);
        match decision {
            // HTTP 202 keeps the session open. A job can arrive on a later poll.
            Admit::Stay => self.stay(workers).await,
            Admit::Hold => self.hold().await,
            Admit::Stop => Ok(true),
            Admit::Error => Err(EnsureError::Unexpected {
                status: 0,
                step: "queue",
            }),
            Admit::Ack { stop } => {
                let acknowledged = ack_ready(
                    self.link,
                    self.set_id,
                    self.session,
                    self.admin_token,
                    path,
                    queue,
                    polled,
                )?;
                self.held_offers.observe_ack(acknowledged);
                if let Some(message_id) = acknowledged {
                    self.last_message_id = message_id;
                }
                Ok(stop)
            }
            Admit::Start { stop } => {
                let outcome = self.start(workers, path, queue, polled, stop).await?;
                self.held_offers
                    .observe_ack(outcome.acknowledged_message_id);
                if let Some(message_id) = outcome.acknowledged_message_id {
                    self.last_message_id = message_id;
                }
                Ok(outcome.stop)
            }
        }
    }

    async fn start(
        &mut self,
        workers: &mut Vec<Started>,
        path: String,
        queue: Option<String>,
        polled: &Poll,
        stop: bool,
    ) -> Result<StartOutcome, EnsureError> {
        let admin = self.link.base().to_owned();
        let queue_token = self.session.token().to_owned();
        let mut lane = super::host_lane::HostLane {
            link: self.link,
            admin,
            queue: queue.clone(),
            queue_path: path.clone(),
            session: Some(self.session),
            set_id: self.set_id,
            admin_token: self.admin_token,
        };
        start_turn(
            &mut lane,
            workers,
            Ready {
                set_id: self.set_id,
                queue_token,
                admin_token: self.admin_token,
                path,
                polled,
            },
            self.journal,
            self.docker,
            self.capacity,
            stop,
        )
        .await
    }

    async fn stay(&self, workers: &[Started]) -> Result<bool, EnsureError> {
        if self.target > self.capacity && workers.len() >= self.capacity as usize {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        Ok(false)
    }

    async fn hold(&self) -> Result<bool, EnsureError> {
        if self.target > self.capacity {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            return Ok(false);
        }
        Ok(true)
    }
}

/// Start one admitted poll through an injectable lane. A conflict remains an
/// error so the current message is redelivered; only a bound worker covers it.
async fn start_turn<T>(
    lane: &mut T,
    workers: &mut Vec<Started>,
    ready: Ready<'_>,
    journal: &Journal,
    docker: &bollard::Docker,
    capacity: u32,
    stop: bool,
) -> Result<StartOutcome, EnsureError>
where
    T: velnor_runner_github::Transport + super::Lane,
{
    let outcome = drive_ready(lane, ready, journal, docker, capacity).await?;
    if let Some(worker) = outcome.started {
        workers.push(worker);
    }
    Ok(StartOutcome {
        stop,
        acknowledged_message_id: outcome.acknowledged_message_id,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct StartOutcome {
    stop: bool,
    acknowledged_message_id: Option<i64>,
}

mod observations;

#[cfg(test)]
mod tests;
